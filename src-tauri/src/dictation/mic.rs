//! The dictation's **microphone, opened on demand** (2026-10-22).
//!
//! Until then Yap held Windows' default microphone open the whole time it
//! ran, for a ~300 ms pre-roll ring that caught a first word said before the
//! key went down. That cost more than it caught: Windows showed the mic in
//! use all day (it looked like "always listening"), a **Bluetooth headset
//! dropped to its call profile (HFP) system-wide** — music in call quality
//! just because Yap was open — and the idle stream was most of Yap's idle
//! CPU.
//!
//! Now the stream opens when a recording needs it and closes after:
//!
//! - **a dictation** holds it from its start (the hotkey's press, the bar's
//!   Dictate, push-to-talk, the rewrite and assistant shortcuts, the Chat
//!   mic) until its stop chime has played;
//! - **a mic test** (onboarding's level meter) holds it while it's on;
//! - afterwards it stays **warm** for [`WARM_MS`] (a minute), so a quick
//!   follow-up starts at once and keeps the pre-roll — **never on
//!   Bluetooth** ([`is_bluetooth`]): a headset's input closes as soon as the
//!   dictation is over, so it goes back to its music profile;
//! - **"Keep the microphone ready"** (`config.keep_mic_ready`, off) holds it
//!   the whole time, as before: instant start and the first word caught,
//!   with the mic shown in use and a Bluetooth headset in call quality.
//!
//! **Which microphone** (2026-10-25, `mics.rs`): the first one in the
//! person's order that's connected, then Windows' default
//! (`mics::candidates`; one that won't open gives way to the next). Windows'
//! list changing (`mics::on_change`) is looked at here: the stream's device
//! gone — or its stream stopping by itself (cpal's error: unplugged) and
//! Windows no longer listing it — is **lost** ([`Sink::lost`]: a dictation
//! recording from it ends and keeps what it heard, `pipeline/unplugged.rs`);
//! a device still listed is opened again once (a reset, a format change). A
//! microphone higher in the order connecting, or the order changing, is used
//! from the next dictation (never in the middle of one): a warm stream on
//! another closes, one held by the mic test or "Keep the microphone ready"
//! moves to it at once.
//!
//! One thread owns the stream ([`Mic`]: cpal streams are `!Send`, and opens
//! and closes then happen one at a time, in order, never on the hotkey's
//! thread). A dictation never waits for the open: it records from the
//! moment it starts, and everything the new stream delivers goes into it
//! (`pipeline.rs`). What it waits for is the **first real audio**
//! ([`Sink::live`]): a sample that isn't digital silence, or
//! [`ZEROS_LIVE_MS`] of silence (a muted mic: the dead-mic watchdog's case) —
//! the start chime and the bar's "speak now" follow that, not the key, so
//! nobody talks into a mic that isn't on yet (a Bluetooth headset takes
//! 0.5–2 s to switch profiles, an asleep one in the order included).
//!
//! The pure parts — what to do next ([`next`]), what's Bluetooth
//! ([`is_bluetooth`]), when a stream is live ([`live_now`]), what a stopped
//! stream means ([`after_stop`]) — are unit-tested; the `--ignored`
//! `open_latency_this_pc` times each input on the PC it runs on. A test run
//! with made-up microphones (`mics::fake`) "opens" them as a tone.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::dictation::mics::{self, MicChoice, Target};
use crate::dictation::pipeline::{resample_linear, TARGET_SAMPLE_RATE};

/// How long the mic stays open after a dictation (or a mic test) unless
/// something needs it: a follow-up within it starts instantly, with the
/// pre-roll. Not for a Bluetooth input (closed at once).
pub(crate) const WARM_MS: u64 = 60_000;
/// A new stream that has delivered this much audio, all of it digital
/// silence, counts as live anyway: a muted mic or one Windows' privacy
/// switches block (the dead-mic watchdog takes it from there).
pub(crate) const ZEROS_LIVE_MS: u64 = 1_500;
const RATE: u64 = TARGET_SAMPLE_RATE as u64;
/// A stream that stopped by itself on a device Windows still lists is
/// opened again — at most once in this long (one that keeps stopping is
/// lost).
const REOPEN_GAP: Duration = Duration::from_secs(3);
/// A device lost while Windows still listed it is passed over for this
/// long (or until Windows' list drops it).
const AWAY_FOR: Duration = Duration::from_secs(10);

/// The warm window, [`WARM_MS`] — or, in a debug build, `YAP_MIC_WARM_MS`
/// (the robot tests' short one).
pub(crate) fn warm_ms() -> u64 {
    #[cfg(debug_assertions)]
    if let Some(ms) = std::env::var("YAP_MIC_WARM_MS").ok().and_then(|v| v.trim().parse().ok()) {
        return ms;
    }
    WARM_MS
}

// ---- the rules (pure) --------------------------------------------------------------------

/// What wants the stream open right now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Holds {
    /// A dictation is recording (or its stop chime is still playing).
    pub dictation: bool,
    /// Onboarding's mic test (its level meter) is on.
    pub test: bool,
    /// "Keep the microphone ready".
    pub keep_ready: bool,
}

impl Holds {
    pub fn any(self) -> bool {
        self.dictation || self.test || self.keep_ready
    }
}

/// What the stream's owner does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Next {
    /// Leave it as it is.
    Stay,
    /// Open it: something needs it and it's closed.
    Open,
    /// Close it now.
    Close,
    /// Close it at this time (ms on the caller's clock), unless something
    /// needs it before then.
    CloseAt(u64),
}

/// The stream's next step: open while anything holds it; with nothing
/// holding it, a Bluetooth input closes at once and any other stays warm
/// until `warm_until` (the end of the last dictation or test plus the warm
/// window; `None`: nothing used it, so it closes now). `now` and
/// `warm_until` are ms on one clock.
pub(crate) fn next(holds: Holds, open: bool, bluetooth: bool, warm_until: Option<u64>, now: u64) -> Next {
    if holds.any() {
        return if open { Next::Stay } else { Next::Open };
    }
    if !open {
        return Next::Stay;
    }
    if bluetooth {
        return Next::Close;
    }
    match warm_until {
        Some(at) if at > now => Next::CloseAt(at),
        _ => Next::Close,
    }
}

/// What a stream that stopped by itself (cpal's error callback), or whose
/// device left Windows' list, means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stopped {
    /// Open the same device again: Windows still lists it (a reset, a
    /// format changed in Sound settings), something holds the mic, and it
    /// hasn't just been reopened.
    Reopen,
    /// It's gone (unplugged, a headset switched off): a dictation from it
    /// ends; the next stream opens on the next microphone.
    Lost,
}

/// See [`Stopped`]. `listed`: Windows lists the device now; `held`:
/// something holds the mic; `reopened_lately`: it was opened again under
/// [`REOPEN_GAP`] ago.
pub(crate) fn after_stop(listed: bool, held: bool, reopened_lately: bool) -> Stopped {
    if listed && held && !reopened_lately {
        Stopped::Reopen
    } else {
        Stopped::Lost
    }
}

/// Is this capture endpoint a Bluetooth headset's? From its device
/// properties: `enumerator` (PKEY_Device_EnumeratorName — "BTHENUM" for a
/// classic headset's hands-free profile, "BTHHFENUM" for Windows' own
/// hands-free driver, "BTHLE…" for LE Audio; "USB", "HDAUDIO", "ROOT"
/// otherwise), `path` (the endpoint's device path, "{1}.BTHENUM\…" or
/// "{2}.\\?\bthhfenum#…") and, should neither say, the friendly `name`
/// Windows gives a classic headset's mic ("Headset (… Hands-Free AG
/// Audio)"). A USB dongle (a Logitech or Jabra headset's receiver) is USB:
/// its mic doesn't change the headset's sound.
pub(crate) fn is_bluetooth(enumerator: &str, path: &str, name: &str) -> bool {
    if enumerator.trim().to_ascii_uppercase().starts_with("BTH") {
        return true;
    }
    let path = path.to_ascii_lowercase();
    if ["bthenum", "bthhfenum", "bthleenum", "bthledevice", "bthhfaud"].iter().any(|m| path.contains(m)) {
        return true;
    }
    let name = name.to_ascii_lowercase();
    name.contains("hands-free") || name.contains("handsfree")
}

/// Has a new stream delivered real audio? `sound`: this chunk has a sample
/// that isn't exactly zero; `samples`: how much it has delivered in all (16
/// kHz) — [`ZEROS_LIVE_MS`] of nothing but zeros counts too.
pub(crate) fn live_now(sound: bool, samples: u64) -> bool {
    sound || samples * 1_000 >= ZEROS_LIVE_MS * RATE
}

// ---- the stream's owner ------------------------------------------------------------------

/// Where the mic's audio and news go (the pipeline's shared state). Called
/// on the audio thread ([`Sink::audio`], [`Sink::live`]) or the mic's own
/// thread (the rest): never blocking.
pub(crate) trait Sink: Send + Sync + 'static {
    /// The mic's audio, 16 kHz mono, as it arrives.
    fn audio(&self, samples: &[f32]);
    /// The stream just delivered its first real audio ([`live_now`]): a
    /// recording waiting for it can say "speak now".
    fn live(&self);
    /// A stream opened on `device` (its name).
    fn opened(&self, device: &str);
    /// An open failed; `dictation`: the lease of the dictation that wanted
    /// it, if one did.
    fn failed(&self, error: &str, dictation: Option<u64>);
    /// The stream's device `device` (its name) went away — unplugged, a
    /// headset switched off; `dictation`: the lease of the dictation
    /// recording from it, if one was.
    fn lost(&self, device: &str, dictation: Option<u64>);
}

/// How the last open went, in ms from when it was asked for — what the log
/// says, and the robot reads (`realtest_mic`).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenTiming {
    pub device: String,
    pub bluetooth: bool,
    /// Building and starting the stream.
    pub open_ms: u64,
    /// The first audio callback.
    pub first_audio_ms: Option<u64>,
    /// Live: the first real audio ([`live_now`]).
    pub live_ms: Option<u64>,
}

/// What the stream's state is, readable from anywhere without the owner.
#[derive(Debug, Default)]
pub(crate) struct State {
    /// A stream is running.
    open: AtomicBool,
    /// An open is under way.
    opening: AtomicBool,
    /// The running stream has delivered real audio.
    live: AtomicBool,
    /// The running stream is a Bluetooth headset's.
    bluetooth: AtomicBool,
    /// Streams opened and closed since Yap started.
    opens: AtomicU64,
    closes: AtomicU64,
    /// Devices lost (unplugged) under a stream.
    losses: AtomicU64,
    last: Mutex<Option<OpenTiming>>,
    /// The running stream's device (its name), if one runs.
    device: Mutex<Option<String>>,
}

/// A snapshot of [`State`] (the test suites' `e2e_mic_status`; debug builds).
#[cfg(debug_assertions)]
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    pub open: bool,
    pub opening: bool,
    pub live: bool,
    pub bluetooth: bool,
    pub opens: u64,
    pub closes: u64,
    pub losses: u64,
    pub last: Option<OpenTiming>,
    /// The device a stream is open on now.
    pub device: Option<String>,
    pub warm_ms: u64,
}

enum Cmd {
    /// A dictation starts (`lease` numbers it; `asked`: when).
    Hold { lease: u64, asked: Instant },
    /// That dictation is over (its stop chime has played): the warm window
    /// starts, or a Bluetooth input closes. `done` hears whether a
    /// Bluetooth stream was closed.
    Release { lease: u64, done: Option<Sender<bool>> },
    /// Onboarding's mic test on or off.
    Test(bool),
    /// Settings: "Keep the microphone ready" and the microphones' order.
    Settings { keep_ready: bool, order: Vec<MicChoice> },
    /// The dead-mic watchdog: stop the stream and open it again.
    Reopen { reply: Sender<Result<(), String>> },
    /// Windows' microphones changed (`mics::on_change`).
    Devices,
    /// Stream `serial` stopped by itself (cpal's error callback).
    Stopped { serial: u64 },
}

/// The dictation's microphone: a handle on the thread that owns the stream.
pub(crate) struct Mic {
    tx: Sender<Cmd>,
    state: Arc<State>,
    lease: AtomicU64,
}

impl Mic {
    /// Start the owner thread; `sink` gets the audio. Nothing opens until
    /// something holds it (or [`Mic::settings`] says keep it ready).
    pub fn start(sink: Weak<dyn Sink>) -> Mic {
        let (tx, rx) = mpsc::channel::<Cmd>();
        let state = Arc::new(State::default());
        let owner_state = Arc::clone(&state);
        let me = tx.clone();
        let spawned = std::thread::Builder::new().name("dictation-mic".into()).spawn(move || {
            let mut owner = Owner {
                sink,
                state: owner_state,
                stream: None,
                holds: Holds::default(),
                lease: None,
                order: Vec::new(),
                away: Vec::new(),
                serial: 0,
                reopened: None,
                me,
                warm_until: None,
                clock: Instant::now(),
            };
            loop {
                let cmd = match owner.deadline() {
                    Some(at) => match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                        Ok(cmd) => Some(cmd),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => break,
                    },
                    None => match rx.recv() {
                        Ok(cmd) => Some(cmd),
                        Err(_) => break,
                    },
                };
                match cmd {
                    Some(cmd) => owner.handle(cmd),
                    // The warm window ran out: anything asked meanwhile (a
                    // dictation starting right now) comes first.
                    None => {
                        while let Ok(cmd) = rx.try_recv() {
                            owner.handle(cmd);
                        }
                    }
                }
                owner.settle();
            }
            owner.close("Yap is closing");
        });
        if let Err(e) = spawned {
            tracing::error!("Microphone: no thread to run it on ({})", e);
        }
        // Windows' microphones changing: looked at on the mic's thread.
        let devices = tx.clone();
        mics::on_change(move || {
            let _ = devices.send(Cmd::Devices);
        });
        Mic { tx, state, lease: AtomicU64::new(0) }
    }

    fn send(&self, cmd: Cmd) {
        if self.tx.send(cmd).is_err() {
            tracing::warn!("Microphone: its thread is gone");
        }
    }

    /// A dictation starts: open the mic if it's closed (in the background:
    /// the recording doesn't wait). Returns the dictation's lease, for
    /// [`Mic::release`].
    pub fn hold(&self) -> u64 {
        let lease = self.lease.fetch_add(1, Ordering::SeqCst) + 1;
        self.send(Cmd::Hold { lease, asked: Instant::now() });
        lease
    }

    /// The dictation `lease` is over (its stop chime has played). Waits,
    /// up to a couple of seconds, for a Bluetooth input to be closed:
    /// `true` when one was (the headset leaves its call profile now).
    pub fn release(&self, lease: u64) -> bool {
        let (done, heard) = mpsc::channel();
        self.send(Cmd::Release { lease, done: Some(done) });
        heard.recv_timeout(Duration::from_secs(2)).unwrap_or(false)
    }

    /// Onboarding's mic test (its level meter) on or off.
    pub fn test(&self, on: bool) {
        self.send(Cmd::Test(on));
    }

    /// "Keep the microphone ready" and the microphones' order (every config
    /// save): a change of order is used from the next dictation.
    pub fn settings(&self, keep_ready: bool, order: Vec<MicChoice>) {
        self.send(Cmd::Settings { keep_ready, order });
    }

    /// The dead-mic watchdog: stop the stream and open it again.
    pub fn reopen(&self) -> Result<(), String> {
        let (reply, answer) = mpsc::channel();
        self.send(Cmd::Reopen { reply });
        answer
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| Err("The microphone didn't answer in time".to_string()))
    }

    /// The running stream has delivered real audio (set by its callback,
    /// cleared when a stream opens or closes).
    pub fn is_live(&self) -> bool {
        self.state.live.load(Ordering::SeqCst)
    }

    /// A stream is running.
    pub fn is_open(&self) -> bool {
        self.state.open.load(Ordering::SeqCst)
    }

    #[cfg(debug_assertions)]
    pub fn status(&self) -> Status {
        let s = &self.state;
        Status {
            open: s.open.load(Ordering::SeqCst),
            opening: s.opening.load(Ordering::SeqCst),
            live: s.live.load(Ordering::SeqCst),
            bluetooth: s.bluetooth.load(Ordering::SeqCst),
            opens: s.opens.load(Ordering::SeqCst),
            closes: s.closes.load(Ordering::SeqCst),
            losses: s.losses.load(Ordering::SeqCst),
            last: s.last.lock().ok().and_then(|l| l.clone()),
            device: s.device.lock().ok().and_then(|d| d.clone()),
            warm_ms: warm_ms(),
        }
    }
}

/// A stream's audio source.
enum Source {
    Cpal(#[allow(dead_code)] cpal::Stream),
    /// A test run's made-up microphone: a tone, stopped when dropped.
    #[cfg(debug_assertions)]
    Fake(#[allow(dead_code)] fake::Tone),
}

/// A running stream.
struct Running {
    _source: Source,
    device: String,
    /// Its Core Audio endpoint id (`None`: opened as Windows' default
    /// without a list to name it).
    endpoint: Option<String>,
    bluetooth: bool,
    /// Numbers the streams, so a stale "stopped" is ignored.
    serial: u64,
}

/// The owner thread's state.
struct Owner {
    sink: Weak<dyn Sink>,
    state: Arc<State>,
    stream: Option<Running>,
    holds: Holds,
    /// The dictation holding it.
    lease: Option<u64>,
    /// The microphones' order (empty: Windows' default).
    order: Vec<MicChoice>,
    /// Devices lost while Windows still listed them, and when.
    away: Vec<(String, Instant)>,
    serial: u64,
    /// When a stream that stopped by itself was last opened again.
    reopened: Option<Instant>,
    /// This thread's own queue (for the streams' "stopped").
    me: Sender<Cmd>,
    warm_until: Option<Instant>,
    /// The clock [`next`]'s ms count on.
    clock: Instant,
}

impl Owner {
    fn ms(&self, at: Instant) -> u64 {
        at.saturating_duration_since(self.clock).as_millis() as u64
    }

    /// When the warm window runs out, if that's what's next.
    fn deadline(&self) -> Option<Instant> {
        match self.plan() {
            Next::CloseAt(_) => self.warm_until,
            _ => None,
        }
    }

    fn plan(&self) -> Next {
        let bluetooth = self.stream.as_ref().is_some_and(|s| s.bluetooth);
        next(
            self.holds,
            self.stream.is_some(),
            bluetooth,
            self.warm_until.map(|t| self.ms(t)),
            self.ms(Instant::now()),
        )
    }

    fn away_ids(&self) -> Vec<String> {
        self.away.iter().map(|(id, _)| id.clone()).collect()
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Hold { lease, asked } => {
                self.lease = Some(lease);
                self.holds.dictation = true;
                self.warm_until = None;
                match &self.stream {
                    // Warm: the recording has the pre-roll and the stream.
                    Some(s) => tracing::info!(bluetooth = s.bluetooth, "Microphone: already open (warm)"),
                    None => self.open(asked),
                }
            }
            Cmd::Release { lease, done } => {
                let mut closed_bluetooth = false;
                if self.lease == Some(lease) {
                    self.lease = None;
                    self.holds.dictation = false;
                    self.warm_until = Some(Instant::now() + Duration::from_millis(warm_ms()));
                    // A Bluetooth headset goes back to its music profile now.
                    if self.plan() == Next::Close {
                        closed_bluetooth = self.stream.as_ref().is_some_and(|s| s.bluetooth);
                        self.close(if closed_bluetooth {
                            "Bluetooth: closed as soon as the dictation ended"
                        } else {
                            "the dictation ended"
                        });
                    } else if let Some(present) = mics::present() {
                        // A microphone higher in the order came meanwhile.
                        self.reconsider(&present);
                    }
                }
                if let Some(done) = done {
                    let _ = done.send(closed_bluetooth);
                }
            }
            Cmd::Test(on) => {
                if on == self.holds.test {
                    return;
                }
                self.holds.test = on;
                if on {
                    self.warm_until = None;
                    if self.stream.is_none() {
                        self.open(Instant::now());
                    }
                } else {
                    self.warm_until = Some(Instant::now() + Duration::from_millis(warm_ms()));
                }
            }
            Cmd::Settings { keep_ready, order } => {
                let reordered = order != self.order;
                self.order = order;
                if keep_ready != self.holds.keep_ready {
                    self.holds.keep_ready = keep_ready;
                    tracing::info!(keep_ready, "Microphone: keep it ready");
                    if keep_ready && self.stream.is_none() {
                        self.open(Instant::now());
                    }
                }
                if reordered {
                    tracing::info!(microphones = self.order.len(), "Microphone: the order changed");
                    if let Some(present) = mics::present() {
                        self.reconsider(&present);
                    }
                }
            }
            Cmd::Reopen { reply } => {
                // The dead stream stops first: two at once would interleave
                // their audio in the recording.
                self.close("to open it again");
                let result = if self.holds.any() {
                    self.open(Instant::now());
                    if self.stream.is_some() {
                        Ok(())
                    } else {
                        Err("no microphone".to_string())
                    }
                } else {
                    Ok(())
                };
                let _ = reply.send(result);
            }
            Cmd::Devices => self.devices(),
            Cmd::Stopped { serial } => self.stopped(serial, false),
        }
    }

    /// Windows' microphones changed: the stream's device gone is lost; a
    /// microphone that comes first now is used from the next dictation.
    fn devices(&mut self) {
        let present = mics::present();
        // A lost device Windows no longer lists can come back; one still
        // listed a while after is usable again.
        self.away.retain(|(id, at)| at.elapsed() < AWAY_FOR && present.as_ref().is_some_and(|l| l.iter().any(|e| &e.id == id)));
        let Some(present) = present else { return };
        let Some(run) = &self.stream else { return };
        if run.endpoint.as_ref().is_some_and(|id| !present.iter().any(|e| &e.id == id)) {
            let serial = run.serial;
            self.stopped(serial, true);
            return;
        }
        self.reconsider(&present);
    }

    /// A stream open on another microphone than the order says now (a
    /// higher one connected, the order changed, Windows' default moved):
    /// never under a dictation. A warm stream closes (the next dictation
    /// opens the right one); one the mic test or "Keep the microphone ready"
    /// holds moves now.
    fn reconsider(&mut self, present: &[mics::Endpoint]) {
        if self.holds.dictation {
            return;
        }
        let Some(open_id) = self.stream.as_ref().and_then(|r| r.endpoint.clone()) else { return };
        let want = mics::in_use(&self.order, present, &self.away_ids());
        if want.as_ref().is_some_and(|w| w.id == open_id) {
            return;
        }
        self.close("another microphone comes first now");
        if self.holds.any() {
            self.open(Instant::now());
        }
    }

    /// Stream `serial` stopped by itself (`gone`: its device left Windows'
    /// list). See [`after_stop`].
    fn stopped(&mut self, serial: u64, gone: bool) {
        let Some(run) = self.stream.as_ref().filter(|r| r.serial == serial) else { return };
        let (device, endpoint) = (run.device.clone(), run.endpoint.clone());
        self.close(if gone { "its device was disconnected" } else { "its stream stopped" });
        // Windows' list now (read again: it may not have caught up yet).
        let listed = if gone {
            None
        } else {
            endpoint
                .as_ref()
                .and_then(|id| mics::refresh().and_then(|l| l.into_iter().find(|e| &e.id == id)))
        };
        let lately = self.reopened.is_some_and(|t| t.elapsed() < REOPEN_GAP);
        if let (Stopped::Reopen, Some(ep)) = (after_stop(listed.is_some(), self.holds.any(), lately), listed.as_ref()) {
            self.reopened = Some(Instant::now());
            match self.build(&Target::Mic(ep.clone()), Instant::now()) {
                Ok(r) => {
                    tracing::info!("Microphone: its stream stopped; opened it again");
                    self.running(r);
                    return;
                }
                Err(e) => tracing::info!("Microphone: couldn't open it again: {}", e),
            }
        }
        if let (Some(id), true) = (endpoint, listed.is_some()) {
            self.away.push((id, Instant::now()));
        }
        self.state.losses.fetch_add(1, Ordering::SeqCst);
        let dictation = self.lease.filter(|_| self.holds.dictation);
        tracing::warn!(recording = dictation.is_some(), "Microphone: disconnected");
        if let Some(sink) = self.sink.upgrade() {
            sink.lost(&device, dictation);
        }
        // The mic test or "Keep the microphone ready": the next one now. (A
        // dictation from it ends; the next opens the next one.)
        if dictation.is_none() && self.holds.any() {
            self.open(Instant::now());
        }
    }

    /// Close it if nothing needs it any more (the warm window ran out; a
    /// Bluetooth input at once).
    fn settle(&mut self) {
        if self.plan() == Next::Close {
            let why = if self.stream.as_ref().is_some_and(|s| s.bluetooth) {
                "Bluetooth: never kept open in the background"
            } else if self.warm_until.is_some() {
                "the warm window ran out"
            } else {
                "nothing needs it"
            };
            self.close(why);
        }
    }

    /// Open the first microphone that will, in order (`mics::candidates`).
    fn open(&mut self, asked: Instant) {
        self.state.opening.store(true, Ordering::SeqCst);
        self.state.live.store(false, Ordering::SeqCst);
        let present = mics::present();
        let targets = mics::candidates(&self.order, present.as_deref(), &self.away_ids());
        let mut error: Option<String> = None;
        for (rank, target) in targets.iter().enumerate() {
            match self.build(target, asked) {
                Ok(running) => {
                    if rank > 0 {
                        tracing::info!(rank, "Microphone: opened one further down the order");
                    }
                    self.running(running);
                    break;
                }
                Err(e) => {
                    tracing::warn!("Microphone: couldn't open it: {}", e);
                    error.get_or_insert(e);
                }
            }
        }
        if self.stream.is_none() {
            let e = error.unwrap_or_else(|| "No microphone found".to_string());
            if targets.is_empty() {
                tracing::warn!("Microphone: none connected");
            }
            if let Some(sink) = self.sink.upgrade() {
                sink.failed(&e, self.lease.filter(|_| self.holds.dictation));
            }
        }
        self.state.opening.store(false, Ordering::SeqCst);
    }

    fn running(&mut self, running: Running) {
        self.state.bluetooth.store(running.bluetooth, Ordering::SeqCst);
        self.state.open.store(true, Ordering::SeqCst);
        self.state.opens.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut d) = self.state.device.lock() {
            *d = Some(running.device.clone());
        }
        if let Some(sink) = self.sink.upgrade() {
            sink.opened(&running.device);
        }
        self.stream = Some(running);
    }

    fn close(&mut self, why: &str) {
        if let Some(running) = self.stream.take() {
            let device = running.device.clone();
            drop(running);
            self.state.open.store(false, Ordering::SeqCst);
            self.state.live.store(false, Ordering::SeqCst);
            self.state.bluetooth.store(false, Ordering::SeqCst);
            self.state.closes.fetch_add(1, Ordering::SeqCst);
            if let Ok(mut d) = self.state.device.lock() {
                *d = None;
            }
            tracing::info!(device = %device, "Microphone closed: {}", why);
        }
    }

    /// Build and start a stream on `target`.
    fn build(&mut self, target: &Target, asked: Instant) -> Result<Running, String> {
        self.serial += 1;
        let serial = self.serial;
        #[cfg(debug_assertions)]
        if let Some(ep) = target.endpoint().filter(|e| mics::fake::is_fake(&e.id)) {
            return Ok(self.fake(ep, asked, serial));
        }
        let started = Instant::now();
        let host = cpal::default_host();
        let device = match target {
            Target::Mic(ep) => host
                .input_devices()
                .map_err(|e| format!("Failed to enumerate input devices: {}", e))?
                .find(|d| d.name().map(|n| n == ep.name).unwrap_or(false))
                .ok_or_else(|| format!("Input device not found: {}", ep.name))?,
            Target::Default => host
                .default_input_device()
                .ok_or_else(|| "No default input device available".to_string())?,
        };
        let dev_name = device.name().unwrap_or_else(|_| "unknown".into());
        let default_config = device
            .default_input_config()
            .map_err(|e| format!("Failed to get default input config: {}", e))?;
        let native_rate = default_config.sample_rate().0;
        let channels = default_config.channels();
        let stream_config = cpal::StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(native_rate),
            buffer_size: cpal::BufferSize::Default,
        };
        let needs_resample = native_rate != TARGET_SAMPLE_RATE;
        let needs_downmix = channels > 1;

        let bluetooth = match target {
            Target::Mic(ep) => ep.bluetooth,
            Target::Default => platform::bluetooth(&dev_name),
        };
        let mut feed = Feed::new(self.sink.clone(), Arc::clone(&self.state), asked);
        let timing = OpenTiming {
            device: dev_name.clone(),
            bluetooth,
            ..Default::default()
        };
        let stopped = self.me.clone();
        let stream = device
            .build_input_stream(
                &stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mono = if needs_downmix {
                        let ch = channels as usize;
                        data.chunks_exact(ch)
                            .map(|frame| frame.iter().sum::<f32>() / ch as f32)
                            .collect::<Vec<f32>>()
                    } else {
                        data.to_vec()
                    };
                    let samples = if needs_resample {
                        resample_linear(&mono, native_rate, TARGET_SAMPLE_RATE)
                    } else {
                        mono
                    };
                    feed.push(&samples);
                },
                // Every error stops cpal's stream (on WASAPI): the device
                // unplugged (DeviceNotAvailable), the audio service gone…
                // The owner decides what it means.
                move |err| {
                    match err {
                        cpal::StreamError::DeviceNotAvailable => tracing::warn!("Microphone: its device is no longer available"),
                        other => tracing::error!("Audio input stream error: {}", other),
                    }
                    let _ = stopped.send(Cmd::Stopped { serial });
                },
                None,
            )
            .map_err(|e| format!("Failed to build input stream: {}", e))?;
        // Timed from here: the callback may run as soon as it plays.
        if let Ok(mut last) = self.state.last.lock() {
            *last = Some(timing);
        }
        stream.play().map_err(|e| format!("Failed to start input stream: {}", e))?;
        let open_ms = started.elapsed().as_millis() as u64;
        if let Ok(mut last) = self.state.last.lock() {
            if let Some(t) = last.as_mut() {
                t.open_ms = open_ms;
            }
        }
        tracing::info!(device = %dev_name, native_rate, channels, bluetooth, open_ms, "Microphone opened");
        Ok(Running {
            _source: Source::Cpal(stream),
            device: dev_name,
            endpoint: target.endpoint().map(|e| e.id.clone()),
            bluetooth,
            serial,
        })
    }

    /// A test run's made-up microphone `ep`: a tone at real-time pace,
    /// stopping by itself (as cpal's stream does) when it's unplugged.
    #[cfg(debug_assertions)]
    fn fake(&mut self, ep: &mics::Endpoint, asked: Instant, serial: u64) -> Running {
        if let Ok(mut last) = self.state.last.lock() {
            *last = Some(OpenTiming { device: ep.name.clone(), bluetooth: ep.bluetooth, ..Default::default() });
        }
        let feed = Feed::new(self.sink.clone(), Arc::clone(&self.state), asked);
        let tone = fake::Tone::start(ep.id.clone(), feed, self.me.clone(), serial);
        tracing::info!(bluetooth = ep.bluetooth, "e2e: a made-up microphone opened");
        Running {
            _source: Source::Fake(tone),
            device: ep.name.clone(),
            endpoint: Some(ep.id.clone()),
            bluetooth: ep.bluetooth,
            serial,
        }
    }
}

/// A stream's audio on its way to the sink, with its first audio and first
/// real audio timed (the audio thread's side of [`OpenTiming`]).
struct Feed {
    sink: Weak<dyn Sink>,
    state: Arc<State>,
    asked: Instant,
    first: bool,
    live: bool,
    delivered: u64,
}

impl Feed {
    fn new(sink: Weak<dyn Sink>, state: Arc<State>, asked: Instant) -> Self {
        Feed { sink, state, asked, first: true, live: false, delivered: 0 }
    }

    fn push(&mut self, samples: &[f32]) {
        if self.first {
            self.first = false;
            if let Ok(mut last) = self.state.last.lock() {
                if let Some(t) = last.as_mut() {
                    t.first_audio_ms = Some(self.asked.elapsed().as_millis() as u64);
                }
            }
        }
        let sink = self.sink.upgrade();
        if let Some(sink) = &sink {
            sink.audio(samples);
        }
        if !self.live {
            self.delivered += samples.len() as u64;
            if live_now(crate::dictation::mic_watch::has_sound(samples), self.delivered) {
                self.live = true;
                self.state.live.store(true, Ordering::SeqCst);
                let ms = self.asked.elapsed().as_millis() as u64;
                if let Ok(mut last) = self.state.last.lock() {
                    if let Some(t) = last.as_mut() {
                        t.live_ms = Some(ms);
                        tracing::info!(
                            device = %t.device,
                            bluetooth = t.bluetooth,
                            open_ms = t.open_ms,
                            first_audio_ms = t.first_audio_ms,
                            live_ms = ms,
                            "Microphone: live"
                        );
                    }
                }
                if let Some(sink) = &sink {
                    sink.live();
                }
            }
        }
    }
}

/// A test run's made-up microphone's stream: a quiet tone on a thread of
/// its own.
#[cfg(debug_assertions)]
mod fake {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::Sender;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::{Cmd, Feed, RATE};

    pub(super) struct Tone {
        stop: Arc<AtomicBool>,
    }

    impl Tone {
        /// Feed a tone from microphone `id` until dropped — or, as a real
        /// stream does when its device goes, until `id` is unplugged
        /// (`mics::fake`): then `Stopped` to the owner.
        pub(super) fn start(id: String, mut feed: Feed, owner: Sender<Cmd>, serial: u64) -> Tone {
            let stop = Arc::new(AtomicBool::new(false));
            let flag = Arc::clone(&stop);
            let _ = std::thread::Builder::new().name("e2e-mic".into()).spawn(move || {
                let started = Instant::now();
                let mut sent: u64 = 0;
                while !flag.load(Ordering::SeqCst) {
                    if !crate::dictation::mics::fake::connected(&id) {
                        tracing::warn!("e2e: the made-up microphone's stream stopped (unplugged)");
                        let _ = owner.send(Cmd::Stopped { serial });
                        break;
                    }
                    let due = started.elapsed().as_millis() as u64 * RATE / 1_000;
                    if due > sent {
                        let chunk: Vec<f32> = (sent..due)
                            .map(|i| 0.05 * (i as f32 * 2.0 * std::f32::consts::PI * 220.0 / RATE as f32).sin())
                            .collect();
                        sent = due;
                        feed.push(&chunk);
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            });
            Tone { stop }
        }
    }

    impl Drop for Tone {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
        }
    }
}

// ---- the endpoint's facts (Windows) ---------------------------------------------------------

#[cfg(windows)]
mod platform {
    /// Is the input named `name` (cpal's name, opened as Windows' default
    /// without a list) a Bluetooth headset's?
    pub(crate) fn bluetooth(name: &str) -> bool {
        let all = crate::dictation::mics::win::capture_endpoints().unwrap_or_default();
        let found = all.iter().find(|f| f.default).or_else(|| all.iter().find(|f| f.name == name));
        match found {
            Some(f) => {
                let bt = super::is_bluetooth(&f.enumerator, &f.path, &f.name);
                tracing::debug!(enumerator = %f.enumerator, form_factor = ?f.form_factor, bluetooth = bt, "Microphone: its endpoint");
                bt
            }
            // Unknown: judged by its name alone.
            None => super::is_bluetooth("", "", name),
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub(crate) fn bluetooth(name: &str) -> bool {
        super::is_bluetooth("", "", name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOLD: Holds = Holds { dictation: true, test: false, keep_ready: false };
    const NONE: Holds = Holds { dictation: false, test: false, keep_ready: false };

    #[test]
    fn a_dictation_opens_it_and_keeps_it_open() {
        assert_eq!(next(HOLD, false, false, None, 0), Next::Open);
        assert_eq!(next(HOLD, true, false, None, 0), Next::Stay);
        // Bluetooth too, while the dictation records.
        assert_eq!(next(HOLD, true, true, None, 0), Next::Stay);
        // So does the mic test, and "Keep the microphone ready".
        let test = Holds { test: true, ..NONE };
        assert_eq!(next(test, false, true, None, 0), Next::Open);
        let ready = Holds { keep_ready: true, ..NONE };
        assert_eq!(next(ready, false, false, None, 0), Next::Open);
        assert_eq!(next(ready, true, true, Some(10), 99_999), Next::Stay);
    }

    #[test]
    fn after_a_dictation_it_stays_warm_for_a_while_then_closes() {
        let end = 5_000;
        let warm = end + WARM_MS;
        assert_eq!(next(NONE, true, false, Some(warm), end), Next::CloseAt(warm));
        assert_eq!(next(NONE, true, false, Some(warm), warm - 1), Next::CloseAt(warm));
        assert_eq!(next(NONE, true, false, Some(warm), warm), Next::Close);
        assert_eq!(next(NONE, true, false, Some(warm), warm + 1), Next::Close);
        // Nothing used it (a switch, keep-ready turned off long after): now.
        assert_eq!(next(NONE, true, false, None, 0), Next::Close);
        // Closed and unneeded: nothing to do.
        assert_eq!(next(NONE, false, false, Some(warm), end), Next::Stay);
    }

    #[test]
    fn a_bluetooth_input_never_stays_warm() {
        assert_eq!(next(NONE, true, true, Some(1_000_000), 0), Next::Close);
        assert_eq!(next(NONE, true, true, None, 0), Next::Close);
    }

    #[test]
    fn a_stopped_stream_is_reopened_only_while_windows_lists_its_device() {
        // A reset or a format change: the device is still there.
        assert_eq!(after_stop(true, true, false), Stopped::Reopen);
        // Unplugged: Windows no longer lists it.
        assert_eq!(after_stop(false, true, false), Stopped::Lost);
        // It keeps stopping: lost after one reopen.
        assert_eq!(after_stop(true, true, true), Stopped::Lost);
        // Nothing holds the mic (a warm stream): no need to open it again.
        assert_eq!(after_stop(true, false, false), Stopped::Lost);
    }

    #[test]
    fn bluetooth_is_read_from_the_endpoint() {
        // Classic hands-free profile, Windows' own hands-free driver, LE Audio.
        assert!(is_bluetooth("BTHENUM", "{1}.BTHENUM\\{0000111E-0000-1000-8000-00805F9B34FB}", "Headset (Contoso Buds)"));
        assert!(is_bluetooth("BTHHFENUM", "", "Headset"));
        assert!(is_bluetooth("bthledevice", "", "Microphone"));
        assert!(is_bluetooth("", "{2}.\\\\?\\bthhfenum#{0000111e-0000-1000-8000-00805f9b34fb}", "Headset"));
        // Older names say it when the properties don't.
        assert!(is_bluetooth("", "", "Headset (Contoso Buds Hands-Free AG Audio)"));
        // USB (a wireless headset's own dongle too), built-in, virtual.
        assert!(!is_bluetooth("USB", "{1}.USB\\VID_046D&PID_0ACB&MI_00\\D&8F5086&0&0000", "Microphone (Contoso Wireless Headset)"));
        assert!(!is_bluetooth("HDAUDIO", "{1}.HDAUDIO\\FUNC_01&VEN_10EC", "Microphone Array (Northwind Audio)"));
        assert!(!is_bluetooth("ROOT", "{1}.ROOT\\STREAMINGMICROPHONE\\0000", "Microphone (Contoso Streaming)"));
        assert!(!is_bluetooth("", "", "Microphone (Fabrikam USB Mic)"));
    }

    #[test]
    fn a_stream_is_live_at_its_first_sound_or_after_a_stretch_of_silence() {
        assert!(live_now(true, 160));
        assert!(!live_now(false, 160));
        // A muted mic: 1.5 s of exact zeros.
        assert!(!live_now(false, ZEROS_LIVE_MS * RATE / 1_000 - 1));
        assert!(live_now(false, ZEROS_LIVE_MS * RATE / 1_000));
    }

    #[test]
    fn the_warm_window_is_a_minute() {
        assert_eq!(WARM_MS, 60_000);
    }

    /// This PC's inputs: each opened cold five times — how long building
    /// and starting the stream takes, its first audio callback, where the
    /// audio in it begins (the callback's time less the audio it carries:
    /// the true start of the capture), and its first real audio (not
    /// digital silence), in ms from asking — and what Windows says about it
    /// (enumerator, Bluetooth). Opens real microphones (Windows shows the
    /// mic in use meanwhile); plays nothing.
    /// `cargo test --release --lib mic::tests::open_latency_this_pc -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn open_latency_this_pc() {
        #[cfg(windows)]
        for f in crate::dictation::mics::win::capture_endpoints().unwrap_or_default() {
            println!(
                "{} — enumerator {:?}, form factor {:?}, default {}, bluetooth {}",
                f.name,
                f.enumerator,
                f.form_factor,
                f.default,
                is_bluetooth(&f.enumerator, &f.path, &f.name)
            );
        }
        let host = cpal::default_host();
        let names: Vec<String> = host
            .input_devices()
            .map(|d| d.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default();
        for name in names {
            let mut rows = Vec::new();
            for _ in 0..5 {
                let device = host.input_devices().unwrap().find(|d| d.name().ok().as_deref() == Some(&name)).unwrap();
                let cfg = device.default_input_config().unwrap();
                let (rate, ch) = (cfg.sample_rate().0, cfg.channels());
                let asked = Instant::now();
                let first = Arc::new(Mutex::new(None::<(u64, u64)>));
                let sound = Arc::new(Mutex::new(None::<u64>));
                let (f2, s2) = (Arc::clone(&first), Arc::clone(&sound));
                let frame_ms = 1_000.0 / rate as f64;
                let stream = device
                    .build_input_stream(
                        &cpal::StreamConfig { channels: ch, sample_rate: cpal::SampleRate(rate), buffer_size: cpal::BufferSize::Default },
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            let ms = asked.elapsed().as_millis() as u64;
                            let mut f = f2.lock().unwrap();
                            if f.is_none() {
                                let carried = (data.len() / ch as usize) as f64 * frame_ms;
                                *f = Some((ms, ms.saturating_sub(carried as u64)));
                            }
                            let mut s = s2.lock().unwrap();
                            if s.is_none() && crate::dictation::mic_watch::has_sound(data) {
                                *s = Some(ms);
                            }
                        },
                        |e| eprintln!("stream error: {e}"),
                        None,
                    )
                    .unwrap();
                stream.play().unwrap();
                let opened = asked.elapsed().as_millis() as u64;
                let until = Instant::now() + Duration::from_millis(3_000);
                while sound.lock().unwrap().is_none() && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(1));
                }
                drop(stream);
                rows.push((opened, *first.lock().unwrap(), *sound.lock().unwrap()));
                std::thread::sleep(Duration::from_millis(500));
            }
            println!("{name}: (open, (first callback, its audio begins), first sound) ms = {rows:?}");
        }
    }
}

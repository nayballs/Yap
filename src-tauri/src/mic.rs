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
//! One thread owns the stream ([`Mic`]: cpal streams are `!Send`, and opens
//! and closes then happen one at a time, in order, never on the hotkey's
//! thread). A dictation never waits for the open: it records from the
//! moment it starts, and everything the new stream delivers goes into it
//! (`pipeline.rs`). What it waits for is the **first real audio**
//! ([`Sink::live`]): a sample that isn't digital silence, or
//! [`ZEROS_LIVE_MS`] of silence (a muted mic: the dead-mic watchdog's case) —
//! the start chime and the bar's "speak now" follow that, not the key, so
//! nobody talks into a mic that isn't on yet (a Bluetooth headset takes
//! 0.5–2 s to switch profiles).
//!
//! The pure parts — what to do next ([`next`]), what's Bluetooth
//! ([`is_bluetooth`]), when a stream is live ([`live_now`]) — are
//! unit-tested; the `--ignored` `open_latency_this_pc` times each input on
//! the PC it runs on.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::pipeline::{resample_linear, TARGET_SAMPLE_RATE};

/// How long the mic stays open after a dictation (or a mic test) unless
/// something needs it: a follow-up within it starts instantly, with the
/// pre-roll. Not for a Bluetooth input (closed at once).
pub(crate) const WARM_MS: u64 = 60_000;
/// A new stream that has delivered this much audio, all of it digital
/// silence, counts as live anyway: a muted mic or one Windows' privacy
/// switches block (the dead-mic watchdog takes it from there).
pub(crate) const ZEROS_LIVE_MS: u64 = 1_500;
const RATE: u64 = TARGET_SAMPLE_RATE as u64;

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
    last: Mutex<Option<OpenTiming>>,
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
    pub last: Option<OpenTiming>,
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
    /// Settings: "Keep the microphone ready" and the microphone.
    Settings { keep_ready: bool, device: Option<String> },
    /// Settings picked another microphone: switch a running stream to it
    /// live (the old one keeps running if the new one fails), else check
    /// it's there.
    Switch { device: Option<String>, reply: Sender<Result<(), String>> },
    /// The dead-mic watchdog: stop the stream and open it again.
    Reopen { reply: Sender<Result<(), String>> },
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
        let spawned = std::thread::Builder::new().name("dictation-mic".into()).spawn(move || {
            let mut owner = Owner {
                sink,
                state: owner_state,
                stream: None,
                holds: Holds::default(),
                lease: None,
                device: None,
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

    /// "Keep the microphone ready" and the configured microphone (every
    /// config save).
    pub fn settings(&self, keep_ready: bool, device: Option<String>) {
        self.send(Cmd::Settings { keep_ready, device });
    }

    /// Settings picked `device`: a running stream switches to it live, else
    /// it's checked that it's there. Errors (the old stream carries on).
    pub fn switch(&self, device: Option<String>) -> Result<(), String> {
        let (reply, answer) = mpsc::channel();
        self.send(Cmd::Switch { device, reply });
        answer
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| Err("The microphone didn't answer in time".to_string()))
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
            last: s.last.lock().ok().and_then(|l| l.clone()),
            warm_ms: warm_ms(),
        }
    }
}

/// A running stream.
struct Running {
    _stream: cpal::Stream,
    device: String,
    bluetooth: bool,
}

/// The owner thread's state.
struct Owner {
    sink: Weak<dyn Sink>,
    state: Arc<State>,
    stream: Option<Running>,
    holds: Holds,
    /// The dictation holding it.
    lease: Option<u64>,
    /// The configured microphone (`None`: Windows' default).
    device: Option<String>,
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
            Cmd::Settings { keep_ready, device } => {
                self.device = device;
                if keep_ready != self.holds.keep_ready {
                    self.holds.keep_ready = keep_ready;
                    tracing::info!(keep_ready, "Microphone: keep it ready");
                    if keep_ready && self.stream.is_none() {
                        self.open(Instant::now());
                    }
                }
            }
            Cmd::Switch { device, reply } => {
                self.device = device.clone();
                let result = match self.stream.take() {
                    // Built before the old one stops: a failure leaves the
                    // old one running.
                    Some(old) => match self.build(device.as_deref(), Instant::now()) {
                        Ok(new) => {
                            drop(old);
                            self.state.closes.fetch_add(1, Ordering::SeqCst);
                            self.running(new);
                            Ok(())
                        }
                        Err(e) => {
                            self.stream = Some(old);
                            Err(e)
                        }
                    },
                    None => present(device.as_deref()),
                };
                let _ = reply.send(result);
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

    fn open(&mut self, asked: Instant) {
        self.state.opening.store(true, Ordering::SeqCst);
        self.state.live.store(false, Ordering::SeqCst);
        let device = self.device.clone();
        match self.build(device.as_deref(), asked) {
            Ok(running) => self.running(running),
            Err(e) => {
                tracing::warn!("Microphone: couldn't open it: {}", e);
                if let Some(sink) = self.sink.upgrade() {
                    sink.failed(&e, self.lease.filter(|_| self.holds.dictation));
                }
            }
        }
        self.state.opening.store(false, Ordering::SeqCst);
    }

    fn running(&mut self, running: Running) {
        self.state.bluetooth.store(running.bluetooth, Ordering::SeqCst);
        self.state.open.store(true, Ordering::SeqCst);
        self.state.opens.fetch_add(1, Ordering::SeqCst);
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
            tracing::info!(device = %device, "Microphone closed: {}", why);
        }
    }

    /// Build and start a stream on `device_name` (`None`: Windows' default).
    fn build(&self, device_name: Option<&str>, asked: Instant) -> Result<Running, String> {
        let started = Instant::now();
        let host = cpal::default_host();
        let device = match device_name {
            Some(name) => host
                .input_devices()
                .map_err(|e| format!("Failed to enumerate input devices: {}", e))?
                .find(|d| d.name().map(|n| n == name).unwrap_or(false))
                .ok_or_else(|| format!("Input device not found: {}", name))?,
            None => host
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

        let bluetooth = platform::bluetooth(&dev_name, device_name.is_none());
        let sink = self.sink.clone();
        let state = Arc::clone(&self.state);
        let timing = OpenTiming {
            device: dev_name.clone(),
            bluetooth,
            ..Default::default()
        };
        let mut delivered: u64 = 0;
        let mut first = true;
        let mut live = false;
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
                    if first {
                        first = false;
                        if let Ok(mut last) = state.last.lock() {
                            if let Some(t) = last.as_mut() {
                                t.first_audio_ms = Some(asked.elapsed().as_millis() as u64);
                            }
                        }
                    }
                    let sink = sink.upgrade();
                    if let Some(sink) = &sink {
                        sink.audio(&samples);
                    }
                    if !live {
                        delivered += samples.len() as u64;
                        if live_now(crate::mic_watch::has_sound(&samples), delivered) {
                            live = true;
                            state.live.store(true, Ordering::SeqCst);
                            let ms = asked.elapsed().as_millis() as u64;
                            if let Ok(mut last) = state.last.lock() {
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
                },
                move |err| tracing::error!("Audio input stream error: {}", err),
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
        Ok(Running { _stream: stream, device: dev_name, bluetooth })
    }
}

/// Is `device` (by name; `None`: the default) there to open?
fn present(device: Option<&str>) -> Result<(), String> {
    let host = cpal::default_host();
    match device {
        Some(name) => host
            .input_devices()
            .map_err(|e| format!("Failed to enumerate input devices: {}", e))?
            .any(|d| d.name().map(|n| n == name).unwrap_or(false))
            .then_some(())
            .ok_or_else(|| format!("Input device not found: {}", name)),
        None => host
            .default_input_device()
            .map(|_| ())
            .ok_or_else(|| "No default input device available".to_string()),
    }
}

// ---- the endpoint's facts (Windows) ---------------------------------------------------------

#[cfg(windows)]
mod platform {
    use windows::core::GUID;
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::Media::Audio::{
        eCapture, eConsole, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PROPVARIANT};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
    };
    use windows::Win32::System::Variant::{VT_LPWSTR, VT_UI4};

    /// PKEY_Device_FriendlyName: "Microphone (Razer Seiren Mini)", the name cpal gives.
    const FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 14,
    };
    /// PKEY_Device_EnumeratorName: "USB", "HDAUDIO", "BTHENUM", "BTHHFENUM"…
    const ENUMERATOR_NAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 24,
    };
    /// The endpoint's device path: "{1}.USB\VID_…", "{1}.BTHENUM\…".
    const DEVICE_PATH: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0xb3f8fa53_0004_438e_9003_51a46e139bfc),
        pid: 2,
    };
    /// PKEY_AudioEndpoint_FormFactor (Microphone 4, Headset 5…), for the log.
    const FORM_FACTOR: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e),
        pid: 0,
    };

    /// What Windows says about a capture endpoint.
    #[derive(Debug, Default, Clone)]
    pub(crate) struct Facts {
        pub name: String,
        pub enumerator: String,
        pub path: String,
        pub form_factor: Option<u32>,
    }

    /// COM on this thread, undone on drop when this did it (cpal may have
    /// set the thread up its own way already: RPC_E_CHANGED_MODE, and COM
    /// works either way).
    struct Com(bool);
    impl Drop for Com {
        fn drop(&mut self) {
            if self.0 {
                // SAFETY: balances this thread's successful CoInitializeEx.
                unsafe { CoUninitialize() };
            }
        }
    }

    fn text(value: &PROPVARIANT) -> Option<String> {
        // SAFETY: read as the type the PROPVARIANT says it holds.
        unsafe {
            let inner = &value.Anonymous.Anonymous;
            if inner.vt == VT_LPWSTR && !inner.Anonymous.pwszVal.is_null() {
                inner.Anonymous.pwszVal.to_string().ok()
            } else {
                None
            }
        }
    }

    fn facts(device: &IMMDevice) -> Option<Facts> {
        // SAFETY: COM calls on this thread; each PROPVARIANT is read, then
        // cleared once.
        unsafe {
            let store = device.OpenPropertyStore(STGM_READ).ok()?;
            let read = |key: &PROPERTYKEY| -> (Option<String>, Option<u32>) {
                match store.GetValue(key) {
                    Ok(mut v) => {
                        let s = text(&v);
                        let inner = &v.Anonymous.Anonymous;
                        let n = (inner.vt == VT_UI4).then(|| inner.Anonymous.ulVal);
                        let _ = PropVariantClear(&mut v);
                        (s, n)
                    }
                    Err(_) => (None, None),
                }
            };
            Some(Facts {
                name: read(&FRIENDLY_NAME).0.unwrap_or_default(),
                enumerator: read(&ENUMERATOR_NAME).0.unwrap_or_default(),
                path: read(&DEVICE_PATH).0.unwrap_or_default(),
                form_factor: read(&FORM_FACTOR).1,
            })
        }
    }

    /// Every active capture endpoint's facts, the default one first.
    pub(crate) fn inputs() -> Vec<Facts> {
        // SAFETY: plain COM calls on this thread; every pointer comes from
        // COM and is released by its wrapper.
        unsafe {
            let _com = Com(CoInitializeEx(None, COINIT_MULTITHREADED).is_ok());
            let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else {
                return Vec::new();
            };
            let mut out = Vec::new();
            if let Some(f) = enumerator.GetDefaultAudioEndpoint(eCapture, eConsole).ok().and_then(|d| facts(&d)) {
                out.push(f);
            }
            if let Ok(all) = enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) {
                for i in 0..all.GetCount().unwrap_or(0) {
                    if let Some(f) = all.Item(i).ok().and_then(|d| facts(&d)) {
                        if !out.iter().any(|o: &Facts| o.name == f.name && o.path == f.path) {
                            out.push(f);
                        }
                    }
                }
            }
            out
        }
    }

    /// Is the input named `name` (cpal's name; `default`: it's Windows'
    /// default input) a Bluetooth headset's?
    pub(crate) fn bluetooth(name: &str, default: bool) -> bool {
        let all = inputs();
        let found = if default { all.first() } else { all.iter().find(|f| f.name == name) };
        let found = found.or_else(|| all.iter().find(|f| f.name == name));
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
    pub(crate) fn bluetooth(name: &str, _default: bool) -> bool {
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
    fn bluetooth_is_read_from_the_endpoint() {
        // Classic hands-free profile, Windows' own hands-free driver, LE Audio.
        assert!(is_bluetooth("BTHENUM", "{1}.BTHENUM\\{0000111E-0000-1000-8000-00805F9B34FB}", "Headset (WH-1000XM4)"));
        assert!(is_bluetooth("BTHHFENUM", "", "Headset"));
        assert!(is_bluetooth("bthledevice", "", "Microphone"));
        assert!(is_bluetooth("", "{2}.\\\\?\\bthhfenum#{0000111e-0000-1000-8000-00805f9b34fb}", "Headset"));
        // Older names say it when the properties don't.
        assert!(is_bluetooth("", "", "Headset (Jabra Elite 85t Hands-Free AG Audio)"));
        // USB (a wireless headset's own dongle too), built-in, virtual.
        assert!(!is_bluetooth("USB", "{1}.USB\\VID_046D&PID_0ACB&MI_00\\D&8F5086&0&0000", "Microphone (G435 Wireless Gaming Headset)"));
        assert!(!is_bluetooth("HDAUDIO", "{1}.HDAUDIO\\FUNC_01&VEN_10EC", "Microphone Array (Realtek(R) Audio)"));
        assert!(!is_bluetooth("ROOT", "{1}.ROOT\\STEAMSTREAMINGMICROPHONE\\0000", "Microphone (Steam Streaming Microphone)"));
        assert!(!is_bluetooth("", "", "Microphone (Razer Seiren Mini)"));
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
        for f in platform::inputs() {
            println!(
                "{} — enumerator {:?}, form factor {:?}, bluetooth {}",
                f.name,
                f.enumerator,
                f.form_factor,
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
                            if s.is_none() && crate::mic_watch::has_sound(data) {
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

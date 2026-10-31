//! Yap's sounds, so you can *hear* what it's doing: a dictation starting
//! and stopping, going hands-free, cancelled, failing or slow, meeting
//! notes starting and stopping, a card on the Yap bar. Each moment's sound
//! comes from the pack chosen in Settings → Dictation → Sounds
//! (`sound/packs.rs`: Classic, Soft, Crisp, Minimal, made from numbers as
//! they play), at the sounds' volume, on the output device chosen there
//! (shared with spoken replies, [`open_output`]), each moment switched on
//! or off by itself.
//!
//! **When they play.**
//! - A dictation's start, stop and cancel ([`play`]) play at once and run
//!   their `Played` afterwards: the other apps go quiet after the start
//!   (`mute.rs`), and after the stop or cancel the mic is let go and their
//!   sound comes back. The start follows the mic's first audio, never the
//!   key (`pipeline/recording.rs` `fire_pending_start`).
//! - The others ([`cue`]) wait for a sound still playing, so two never
//!   overlap; a card's sound never plays while a dictation records, nor
//!   right after another sound (its error's, say).
//! - **Never into a meeting's notes**: while a meeting records, no sound
//!   plays (the loopback would carry it into "Them", as spoken replies keep
//!   quiet then: `tts::meeting_quiet`) — except a dictation's start and
//!   stop, which always have (a dictation mid-meeting needs its "speak
//!   now"). Meeting notes' own start plays before the recording opens the
//!   loopback ([`meeting_started`]), its stop once the loopback has closed
//!   ([`meeting_stopped`]).
//!
//! **Test mode** (`e2e.rs`) opens no output device: what would play is kept
//! for the debug-only [`sound_e2e_heard`], and a sound "lasts" its length.
//! Robot mode plays them for real.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

pub mod packs;

pub use packs::{Cue, Pack};
use packs::{MAX_GAIN, SAMPLE_RATE};

/// Run once a sound has played (on the sound's thread), or straight away
/// when it can't play: the other apps go quiet after the start sound and
/// come back after the stop sound (`mute.rs`).
pub type Played = Box<dyn FnOnce() + Send + 'static>;

/// What config.json says about sounds, kept here for every caller
/// ([`sync`] on every save).
#[derive(Debug, Clone, PartialEq)]
struct Settings {
    on: bool,
    pack: Pack,
    volume: f32,
    device: Option<String>,
    moments: [bool; Cue::ALL.len()],
}

impl Settings {
    fn of(cfg: &crate::config::YapConfig) -> Settings {
        Settings {
            on: cfg.sound_enabled,
            pack: Pack::named(&cfg.sound_pack),
            volume: cfg.audio_feedback_volume,
            device: cfg.output_device.clone(),
            moments: Cue::ALL.map(|c| moment_on(cfg, c)),
        }
    }

    /// `cue`'s sound, if it's on: its pack and its peak.
    fn wants(&self, cue: Cue) -> Option<(Pack, f32)> {
        let i = Cue::ALL.iter().position(|c| *c == cue)?;
        let peak = MAX_GAIN * self.volume.clamp(0.0, 1.0);
        (self.on && self.moments[i] && peak > 0.0).then_some((self.pack, peak))
    }
}

/// Whether `cue` has its sound in `cfg` (its own switch; the master switch
/// aside).
pub fn moment_on(cfg: &crate::config::YapConfig, cue: Cue) -> bool {
    match cue {
        Cue::Start => cfg.sound_start,
        Cue::Stop => cfg.sound_stop,
        Cue::Locked => cfg.sound_locked,
        Cue::Cancelled => cfg.sound_cancelled,
        Cue::Error => cfg.sound_error,
        Cue::Slow => cfg.sound_slow,
        Cue::MeetingStart | Cue::MeetingStop => cfg.sound_meeting,
        Cue::Notice => cfg.sound_notice,
    }
}

static SETTINGS: RwLock<Option<Settings>> = RwLock::new(None);

/// Settings saved (and at start-up): what plays from now on.
pub fn sync(cfg: &crate::config::YapConfig) {
    *SETTINGS.write().unwrap_or_else(|p| p.into_inner()) = Some(Settings::of(cfg));
}

fn settings() -> Settings {
    SETTINGS
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .unwrap_or_else(|| Settings::of(&crate::config::YapConfig::default()))
}

/// When the sound playing now ends (`None`: none has played yet).
static PLAYING_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

/// The longest a sound waits for the one before it.
const WAIT_LIMIT: Duration = Duration::from_millis(600);
/// A card's sound isn't played this soon after another sound: that sound
/// said it (an error's, say, before its alert's card).
const NOTICE_AFTER: Duration = Duration::from_millis(1_500);

/// A sound `len` long plays from `delay` on: noted, so the next one waits.
fn note_playing(delay: Duration, len: Duration) {
    let end = Instant::now() + delay + len;
    let mut until = PLAYING_UNTIL.lock().unwrap_or_else(|p| p.into_inner());
    *until = Some(until.map_or(end, |u| u.max(end)));
}

/// How long a sound starting now waits for the one playing.
fn wait_for_playing() -> Duration {
    let until = *PLAYING_UNTIL.lock().unwrap_or_else(|p| p.into_inner());
    until.map_or(Duration::ZERO, |u| u.saturating_duration_since(Instant::now()).min(WAIT_LIMIT))
}

/// A sound played or is playing within [`NOTICE_AFTER`].
fn sounded_lately() -> bool {
    let until = *PLAYING_UNTIL.lock().unwrap_or_else(|p| p.into_inner());
    until.is_some_and(|u| u + NOTICE_AFTER > Instant::now())
}

/// Whether `cue` may play now as far as a meeting goes: nothing while one
/// records, but a dictation's start and stop (see the module's notes).
pub fn meeting_allows(cue: Cue, meeting_recording: bool) -> bool {
    !meeting_recording || matches!(cue, Cue::Start | Cue::Stop)
}

/// A dictation's sound, now: its start, stop or cancel. `played` runs once
/// it has played — or at once (on a thread) when it's off.
pub fn play(cue: Cue, played: Option<Played>) {
    let s = settings();
    match s.wants(cue).filter(|_| meeting_allows(cue, crate::meetings::meeting::is_recording())) {
        Some((pack, peak)) => {
            note_playing(Duration::ZERO, pack.length(cue));
            spawn(pack, cue, peak, s.device, Duration::ZERO, played);
        }
        None => {
            if let Some(f) = played {
                thread::spawn(f);
            }
        }
    }
}

/// One of the other moments' sound, if it's on: after the sound playing
/// now, never while a meeting records; a card's never while a dictation
/// records or right after another sound.
pub fn cue(cue: Cue) {
    let s = settings();
    let Some((pack, peak)) = s.wants(cue) else { return };
    if !meeting_allows(cue, crate::meetings::meeting::is_recording()) {
        tracing::debug!(cue = cue.id(), "Sound: not while a meeting records");
        return;
    }
    if cue == Cue::Notice && (sounded_lately() || crate::dictation::pipeline::dictation_state().1) {
        return;
    }
    let delay = wait_for_playing();
    note_playing(delay, pack.length(cue));
    spawn(pack, cue, peak, s.device, delay, None);
}

/// Meeting notes are starting (`meeting::start`, the slot claimed, before
/// the capture opens): their sound, if it's on, played to the end before
/// this returns (a moment), so the loopback never hears it.
pub fn meeting_started() {
    let s = settings();
    let Some((pack, peak)) = s.wants(Cue::MeetingStart) else { return };
    let len = pack.length(Cue::MeetingStart);
    note_playing(Duration::ZERO, len);
    if simulated() {
        heard(pack, Cue::MeetingStart, s.volume, false);
        thread::sleep(len);
        return;
    }
    output(pack.render(Cue::MeetingStart, peak), s.device.as_deref(), len + Duration::from_millis(500), &|| false);
}

/// Meeting notes were asked to stop (`meeting::stop`): their sound, if
/// it's on, once the capture has closed its streams (the loopback with
/// them: `meeting::capturing`, a fraction of a second).
pub fn meeting_stopped() {
    let s = settings();
    let Some((pack, peak)) = s.wants(Cue::MeetingStop) else { return };
    let _ = thread::Builder::new().name("sound-meeting-stop".into()).spawn(move || {
        let asked = Instant::now();
        while crate::meetings::meeting::capturing() {
            if asked.elapsed() > Duration::from_secs(3) {
                tracing::info!("Sound: the meeting's capture didn't close; no stop sound");
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let delay = wait_for_playing();
        note_playing(delay, pack.length(Cue::MeetingStop));
        play_here(pack, Cue::MeetingStop, peak, s.volume, s.device.as_deref(), delay, None);
    });
}

/// Plays `cue` on a thread of its own after `delay`, then `played`.
fn spawn(pack: Pack, cue: Cue, peak: f32, device: Option<String>, delay: Duration, played: Option<Played>) {
    let volume = peak / MAX_GAIN;
    // (`thread::spawn`, as ever: `played` — the mic let go, the other apps'
    // sound back — must never be dropped unrun with a thread that failed.)
    thread::spawn(move || play_here(pack, cue, peak, volume, device.as_deref(), delay, played));
}

/// Plays `cue` on this thread after `delay`, then runs `played`.
fn play_here(pack: Pack, cue: Cue, peak: f32, volume: f32, device: Option<&str>, delay: Duration, played: Option<Played>) {
    if !delay.is_zero() {
        thread::sleep(delay);
    }
    let len = pack.length(cue);
    if simulated() {
        heard(pack, cue, volume, false);
        thread::sleep(len);
    } else {
        output(pack.render(cue, peak), device, PLAY_LIMIT, &|| false);
    }
    if let Some(f) = played {
        f();
    }
}

/// Plays `samples` on the output device and waits until they have played
/// (`limit` at most, or until `stop` says so).
fn output(samples: Vec<f32>, device: Option<&str>, limit: Duration, stop: &dyn Fn() -> bool) {
    use rodio::buffer::SamplesBuffer;
    use rodio::Sink;
    if let Some((_stream, handle)) = open_output(device) {
        if let Ok(sink) = Sink::try_new(&handle) {
            sink.append(SamplesBuffer::new(1, SAMPLE_RATE, samples));
            wait_played_or(&sink, limit, stop);
        }
    }
}

/// Open an output stream on the named device, falling back to the system
/// default when no name is given or the device can't be found. Uses rodio's
/// re-exported cpal so the device type matches `OutputStream::try_from_device`.
pub(crate) fn open_output(device: Option<&str>) -> Option<(rodio::OutputStream, rodio::OutputStreamHandle)> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};

    if let Some(name) = device {
        let host = rodio::cpal::default_host();
        if let Ok(mut devs) = host.output_devices() {
            if let Some(dev) = devs.find(|d| d.name().map(|n| n == name).unwrap_or(false)) {
                if let Ok(pair) = rodio::OutputStream::try_from_device(&dev) {
                    return Some(pair);
                }
                // Its name at debug only: a headset's can carry its owner's.
                tracing::warn!("Failed to open the sounds' output device; using default");
                tracing::debug!(device = name, "Sounds' output device that wouldn't open");
            }
        }
    }
    rodio::OutputStream::try_default().ok()
}

/// The longest a sound is waited for: it lasts under a third of a second,
/// so past this its output has stopped taking audio.
const PLAY_LIMIT: Duration = Duration::from_secs(3);

/// Wait until `sink` has played what it holds, or `limit` at most. An
/// output device that goes while a sound plays (headphones unplugged, a
/// Bluetooth headset switched off with its mic, mid-dictation) stops its
/// stream, and nothing drains the sink again: rodio's `sleep_until_end`
/// waited for ever then, and what runs after the stop sound — the
/// dictation's mic let go, the other apps' sound back — never did.
#[cfg(test)]
fn wait_played(sink: &rodio::Sink, limit: Duration) {
    wait_played_or(sink, limit, &|| false)
}

/// [`wait_played`], cut short (and the sink emptied) once `stop` says so: a
/// preview replaced.
fn wait_played_or(sink: &rodio::Sink, limit: Duration, stop: &dyn Fn() -> bool) {
    let started = Instant::now();
    while !sink.empty() {
        if stop() {
            sink.stop();
            return;
        }
        if started.elapsed() >= limit {
            tracing::warn!("Sound: its output stopped taking audio; going on without it");
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

// ---- previews (Settings' ▶) --------------------------------------------------------

/// The preview playing now (a newer one, or ■, stops it).
static PREVIEW: AtomicU64 = AtomicU64::new(0);
/// Between a pack's sounds when its set plays.
const PREVIEW_GAP: f32 = 0.35;

/// What a preview of `pack` plays (`cues`, or its whole set) and how long
/// it lasts.
fn preview_set(pack: Pack, cues: Option<Vec<Cue>>) -> (Vec<Cue>, Duration) {
    let cues = cues.filter(|c| !c.is_empty()).unwrap_or_else(|| Cue::ALL.to_vec());
    let gaps = Duration::from_secs_f32(PREVIEW_GAP * cues.len().saturating_sub(1) as f32);
    let len = cues.iter().map(|c| pack.length(*c)).sum::<Duration>() + gaps;
    (cues, len)
}

/// Settings' ▶: `pack`'s whole set, one after another (or only `cues`'
/// sounds: a moment's), at `volume` (else the sounds' own), whatever the
/// switches say. Never while a meeting records. Returns how long it lasts
/// (ms).
#[tauri::command]
pub fn sound_preview(pack: String, cues: Option<Vec<String>>, volume: Option<f32>) -> Result<u64, String> {
    if crate::meetings::meeting::is_recording() {
        return Err("Yap plays no sounds while it records a meeting".into());
    }
    let s = settings();
    let pack = Pack::named(&pack);
    let cues = match cues {
        Some(ids) => Some(ids.iter().map(|id| Cue::from_id(id).ok_or("No such sound")).collect::<Result<Vec<_>, _>>()?),
        None => None,
    };
    let volume = volume.filter(|v| v.is_finite()).unwrap_or(s.volume).clamp(0.0, 1.0);
    let (cues, len) = preview_set(pack, cues);
    let me = PREVIEW.fetch_add(1, Ordering::SeqCst) + 1;
    tracing::info!(pack = pack.id, sounds = cues.len(), "Sound: preview");
    if simulated() {
        for c in &cues {
            heard(pack, *c, volume, true);
        }
        return Ok(len.as_millis() as u64);
    }
    let device = s.device;
    let _ = thread::Builder::new().name("sound-preview".into()).spawn(move || {
        let gap = vec![0.0f32; (PREVIEW_GAP * SAMPLE_RATE as f32) as usize];
        let mut samples = Vec::new();
        for (i, c) in cues.iter().enumerate() {
            if i > 0 {
                samples.extend_from_slice(&gap);
            }
            samples.extend(pack.render(*c, MAX_GAIN * volume));
        }
        output(samples, device.as_deref(), len + PLAY_LIMIT, &|| PREVIEW.load(Ordering::SeqCst) != me);
    });
    Ok(len.as_millis() as u64)
}

/// Settings' ■: the preview playing stops.
#[tauri::command]
pub fn sound_preview_stop() {
    PREVIEW.fetch_add(1, Ordering::SeqCst);
}

// ---- test mode ---------------------------------------------------------------------

/// A test run's sounds are kept, never played (robot mode plays them).
fn simulated() -> bool {
    crate::shell::e2e::active() && !crate::shell::realtest::on()
}

/// A sound that played (test mode: [`sound_e2e_heard`]).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Heard {
    /// "soft.locked".
    pub id: String,
    pub pack: &'static str,
    pub cue: Cue,
    /// The volume it played at (0.0–1.0).
    pub volume: f32,
    /// Settings' ▶.
    pub preview: bool,
}

static HEARD: Mutex<Vec<Heard>> = Mutex::new(Vec::new());

fn heard(pack: Pack, cue: Cue, volume: f32, preview: bool) {
    tracing::info!(sound = %format!("{}.{}", pack.id, cue.id()), preview, "Sound: played");
    if crate::shell::e2e::active() {
        HEARD.lock().unwrap_or_else(|p| p.into_inner()).push(Heard {
            id: format!("{}.{}", pack.id, cue.id()),
            pack: pack.id,
            cue,
            volume,
            preview,
        });
    }
}

/// Debug-only, test mode: every sound that played (or would have), oldest
/// first; `clear` empties the record after reading it.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn sound_e2e_heard(clear: Option<bool>) -> Result<Vec<Heard>, String> {
    if !crate::shell::e2e::active() {
        return Err("sound_e2e_heard is for test runs".into());
    }
    let mut h = HEARD.lock().unwrap_or_else(|p| p.into_inner());
    let out = h.clone();
    if clear.unwrap_or(false) {
        h.clear();
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;

    /// The sound's output stopped taking audio (its device unplugged while
    /// it played): nothing drains the sink again. `sleep_until_end` waited
    /// for ever, so the stop sound's `played` — the mic let go, the other
    /// apps' sound back — never ran. Now the wait gives up.
    #[test]
    fn a_sound_whose_output_stopped_is_waited_for_a_moment_only() {
        // A sink nothing plays from: its output gone.
        let (sink, _stopped_output) = rodio::Sink::new_idle();
        sink.append(SamplesBuffer::new(1, SAMPLE_RATE, packs::CLASSIC.render(Cue::Stop, MAX_GAIN)));
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            wait_played(&sink, Duration::from_millis(300));
            let _ = tx.send(());
        });
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok(), "the wait gave up");

        // An output that plays it: done as soon as it has.
        let (sink, mut output) = rodio::Sink::new_idle();
        sink.append(SamplesBuffer::new(1, SAMPLE_RATE, packs::CLASSIC.render(Cue::Start, MAX_GAIN)));
        let player = thread::spawn(move || while output.next().is_some() {});
        let started = Instant::now();
        wait_played(&sink, Duration::from_secs(10));
        assert!(started.elapsed() < Duration::from_secs(5), "played, not timed out");
        drop(sink);
        let _ = player.join();
    }

    /// A preview replaced (another ▶, or ■) stops at once.
    #[test]
    fn a_preview_replaced_stops_at_once() {
        let (sink, _idle) = rodio::Sink::new_idle();
        sink.append(SamplesBuffer::new(1, SAMPLE_RATE, packs::SOFT.render(Cue::Notice, MAX_GAIN)));
        let started = Instant::now();
        wait_played_or(&sink, Duration::from_secs(10), &|| true);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    /// New installs: start, stop, hands-free and cancelled on, the rest
    /// off, in Classic at full volume; the master switch off plays none,
    /// and so does no volume.
    #[test]
    fn the_moments_on_by_default_and_the_switches() {
        let mut cfg = crate::config::YapConfig::default();
        let s = Settings::of(&cfg);
        assert_eq!(s.pack, packs::CLASSIC);
        let on: Vec<Cue> = Cue::ALL.into_iter().filter(|c| s.wants(*c).is_some()).collect();
        assert_eq!(on, [Cue::Start, Cue::Stop, Cue::Locked, Cue::Cancelled]);
        assert_eq!(s.wants(Cue::Start), Some((packs::CLASSIC, MAX_GAIN)));

        cfg.sound_meeting = true;
        cfg.sound_pack = "minimal".into();
        cfg.audio_feedback_volume = 0.5;
        let s = Settings::of(&cfg);
        assert_eq!(s.wants(Cue::MeetingStart), Some((packs::MINIMAL, MAX_GAIN * 0.5)));
        assert!(s.wants(Cue::MeetingStop).is_some(), "one switch for both");

        cfg.audio_feedback_volume = 0.0;
        assert_eq!(Settings::of(&cfg).wants(Cue::Start), None);
        cfg.audio_feedback_volume = 1.0;
        cfg.sound_enabled = false;
        let s = Settings::of(&cfg);
        assert!(Cue::ALL.into_iter().all(|c| s.wants(c).is_none()));
    }

    /// While a meeting records only a dictation's start and stop play.
    #[test]
    fn nothing_but_a_dictations_start_and_stop_while_a_meeting_records() {
        for cue in Cue::ALL {
            assert!(meeting_allows(cue, false), "{cue:?}");
            assert_eq!(meeting_allows(cue, true), matches!(cue, Cue::Start | Cue::Stop), "{cue:?}");
        }
    }

    /// A pack's ▶ plays its whole set, in Settings' order, with gaps.
    #[test]
    fn a_packs_preview_is_its_whole_set() {
        let (cues, len) = preview_set(packs::CRISP, None);
        assert_eq!(cues, Cue::ALL);
        let sounds: Duration = Cue::ALL.iter().map(|c| packs::CRISP.length(*c)).sum();
        assert!(len > sounds && len < sounds + Duration::from_secs(4));
        let (one, len) = preview_set(packs::CRISP, Some(vec![Cue::Error]));
        assert_eq!(one, [Cue::Error]);
        assert_eq!(len, packs::CRISP.length(Cue::Error));
        let (two, _) = preview_set(packs::CRISP, Some(vec![Cue::MeetingStart, Cue::MeetingStop]));
        assert_eq!(two, [Cue::MeetingStart, Cue::MeetingStop]);
    }
}

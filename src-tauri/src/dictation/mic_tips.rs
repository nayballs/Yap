//! **Mic-quality tips** (2026-10-29; Wispr Flow's "didn't hear you clearly"
//! and its AirPods advice, by behaviour): after a dictation whose signal was
//! weak, one concrete tip, rarely.
//!
//! - **The signal** ([`measure`], pure): the dictation's audio in 20 ms
//!   frames, each one's level in dBFS. Its loudest half second is the
//!   voice — however much of the recording is silence: a spoken "snip"
//!   keeps listening while the box is drawn and for the question after it
//!   (a share of the frames, the 90th percentile, called a clear voice
//!   quiet there) —, the quiet frames (the 10th percentile) the room; and
//!   how many samples hit full scale.
//! - **What's wrong** ([`trouble`], pure): **clipped** (≥ 0.1 % of samples
//!   at full scale: too loud, distorted), **quiet** (the voice under
//!   −42 dBFS: far from the mic, or its level turned down) or **noisy** (the
//!   voice under 12 dB above a room louder than −50 dBFS). A dictation under
//!   2 s, or one with no voice in it at all (the dead-mic watchdog's), is
//!   never judged.
//! - **One tip** ([`pick`], pure): a Bluetooth headset in hands-free mode
//!   (HFP: its mic is 8 or 16 kHz) gets "Bluetooth headsets sound worse
//!   while their mic is on: try the PC's mic" when another mic is
//!   connected; too loud, "turn its level down in Windows"; quiet, "move
//!   closer, or turn its level up in Windows"; noisy, the next mic in the
//!   order, or "move closer".
//! - **Rarely** ([`eligible`], pure): one mic tip a day at most, each kind
//!   at most [`MAX_SHOWS`] times and not twice in a week. Through the notice
//!   policy, as a tip ("micTip": "Show tips" turns it off, never over a
//!   dictation or a meeting, dropped if unseen): the main window's toast
//!   while the person looks at it, else a Yap bar card — never a Windows
//!   notification. Counts in `<data>/mic_tips.json`.
//!
//! Test mode judges nothing unless a spec turns it on (`e2e_mic_tips`).

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

/// 20 ms at 16 kHz.
const FRAME: usize = 320;
/// The voice is the loudest this many frames (half a second).
const VOICE_FRAMES: usize = 25;
/// Shorter dictations aren't judged.
pub const MIN_SECS: f32 = 2.0;
/// The voice under this (dBFS, its loud frames) is quiet.
pub const QUIET_DB: f32 = -42.0;
/// The voice less than this above the room is noisy…
pub const NOISY_GAP_DB: f32 = 12.0;
/// …when the room is louder than this.
pub const NOISY_FLOOR_DB: f32 = -50.0;
/// A sample this close to full scale is clipped…
const CLIP_LEVEL: f32 = 0.98;
/// …and this share of them is a clipped dictation.
pub const CLIPPED_SHARE: f32 = 0.001;
/// No voice at all under this: nothing to judge.
const SILENT_DB: f32 = -65.0;

/// Each kind of tip shows this many times at most…
pub const MAX_SHOWS: u32 = 3;
/// …not twice within a week…
pub const SAME_GAP_MS: u64 = 7 * 24 * 3_600_000;
/// …and one mic tip a day at most.
pub const MIN_GAP_MS: u64 = 24 * 3_600_000;

/// The main window's event: a [`Shown`].
const EVENT: &str = "yap-mic-tip";
/// The bar card's id and its rule in `notice.rs`.
pub const KIND: &str = "micTip";
const CARD_MS: u64 = 12_000;

// ---- the signal (pure) --------------------------------------------------------------------------

/// A dictation's signal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Signal {
    pub secs: f32,
    /// The voice: its loudest half second ([`VOICE_FRAMES`]; dBFS).
    pub voice_db: f32,
    /// The room: the 10th percentile (dBFS).
    pub floor_db: f32,
    /// The share of samples at full scale.
    pub clipped: f32,
}

fn db(rms: f32) -> f32 {
    20.0 * rms.max(1e-9).log10()
}

/// `samples` (16 kHz mono) measured; `None` under [`MIN_SECS`].
pub fn measure(samples: &[f32]) -> Option<Signal> {
    let secs = samples.len() as f32 / 16_000.0;
    if secs < MIN_SECS {
        return None;
    }
    let mut levels: Vec<f32> = samples
        .as_chunks::<FRAME>()
        .0
        .iter()
        .map(|f| db((f.iter().map(|s| s * s).sum::<f32>() / FRAME as f32).sqrt()))
        .collect();
    levels.sort_by(|a, b| a.total_cmp(b));
    let at = |p: f32| levels[((levels.len() - 1) as f32 * p).round() as usize];
    let clipped = samples.iter().filter(|s| s.abs() >= CLIP_LEVEL).count() as f32 / samples.len() as f32;
    let voice_db = levels[levels.len().saturating_sub(VOICE_FRAMES)];
    Some(Signal { secs, voice_db, floor_db: at(0.1), clipped })
}

/// What was wrong with a dictation's signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Trouble {
    /// Too loud: the voice hit full scale and distorted.
    Clipped,
    /// The voice came through very quietly.
    Quiet,
    /// The room was nearly as loud as the voice.
    Noisy,
}

/// What, if anything, was wrong (the worst first).
pub fn trouble(s: &Signal) -> Option<Trouble> {
    if s.voice_db < SILENT_DB {
        return None;
    }
    if s.clipped >= CLIPPED_SHARE {
        return Some(Trouble::Clipped);
    }
    if s.voice_db < QUIET_DB {
        return Some(Trouble::Quiet);
    }
    if s.floor_db > NOISY_FLOOR_DB && s.voice_db - s.floor_db < NOISY_GAP_DB {
        return Some(Trouble::Noisy);
    }
    None
}

// ---- the tip (pure) -----------------------------------------------------------------------------

/// The dictation's microphone, as the mic's thread had it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MicInfo {
    pub name: String,
    pub endpoint: Option<String>,
    pub bluetooth: bool,
    /// Its own sample rate (Hz; 0 unknown).
    pub rate: u32,
}

/// A Bluetooth headset in hands-free mode: its mic only works in the call
/// profile (HFP), at 8 or 16 kHz. (A Bluetooth LE Audio mic at a higher
/// rate isn't.)
pub fn hands_free(m: &MicInfo) -> bool {
    m.bluetooth && (m.rate == 0 || m.rate <= 16_000)
}

/// Another microphone to suggest: its name, and whether it's the PC's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Other {
    pub name: String,
    pub built_in: bool,
}

/// A tip's kind (its count in `mic_tips.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Bluetooth,
    Clipped,
    Quiet,
    Noisy,
}

/// A tip to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tip {
    pub kind: Kind,
    pub title: String,
    pub body: String,
    /// Its one button: `alert::OPEN_SOUND_SETTINGS` or `alert::OPEN_MIC_ORDER`.
    pub action: &'static str,
    pub label: &'static str,
}

const TITLE: &str = "Yap didn't hear you clearly";

/// The tip for a dictation on `mic` whose signal had `trouble` (if any),
/// with `other` the microphone to suggest instead (a connected one that
/// isn't a Bluetooth headset, the PC's own first). Pure.
pub fn pick(trouble: Option<Trouble>, mic: Option<&MicInfo>, other: Option<&Other>) -> Option<Tip> {
    let sound = |kind, body: String| Tip { kind, title: TITLE.into(), body, action: crate::notices::alert::OPEN_SOUND_SETTINGS, label: "Sound settings" };
    let mics = |kind, title: &str, body: String| Tip { kind, title: title.into(), body, action: crate::notices::alert::OPEN_MIC_ORDER, label: "Microphones" };
    let them = |o: &Other| if o.built_in { "the PC's mic".to_string() } else { crate::dictation::mics::short_name(&o.name) };
    if let (Some(m), Some(o)) = (mic, other) {
        if hands_free(m) {
            let title = if trouble.is_some() { TITLE } else { "Bluetooth headsets sound worse while their mic is on" };
            let body = if trouble.is_some() {
                format!("Bluetooth headsets sound worse while their mic is on: try {}.", them(o))
            } else {
                format!("Try {} for dictation: put it first in Microphones.", them(o))
            };
            return Some(mics(Kind::Bluetooth, title, body));
        }
    }
    match trouble? {
        Trouble::Clipped => Some(sound(
            Kind::Clipped,
            "Your microphone is set so loud that your words distort. Turn its level down in Windows' sound settings.".into(),
        )),
        Trouble::Quiet => Some(sound(
            Kind::Quiet,
            "Your voice came through very quietly. Move closer to the microphone, or turn its level up in Windows' sound settings.".into(),
        )),
        Trouble::Noisy => Some(match other {
            Some(o) => mics(
                Kind::Noisy,
                TITLE,
                format!("There was a lot of noise around your voice. Try {}: put it first in Microphones.", them(o)),
            ),
            None => sound(Kind::Noisy, "There was a lot of noise around your voice. Move closer to the microphone, or somewhere quieter.".into()),
        }),
    }
}

/// The microphone to suggest instead of the one in use (`using`, its
/// endpoint id): a connected one that isn't a Bluetooth headset or a
/// software device, the PC's own first, then the person's order. Pure.
pub fn other_mic(present: &[crate::dictation::mics::Endpoint], order: &[crate::dictation::mics::MicChoice], using: Option<&str>) -> Option<Other> {
    let usable: Vec<&crate::dictation::mics::Endpoint> =
        present.iter().filter(|e| !e.bluetooth && !e.is_virtual && Some(e.id.as_str()) != using).collect();
    let pick = usable
        .iter()
        .find(|e| e.built_in)
        .or_else(|| usable.iter().min_by_key(|e| crate::dictation::mics::rank_of(order, e, present).unwrap_or(usize::MAX)))?;
    Some(Other { name: pick.name.clone(), built_in: pick.built_in })
}

// ---- how often (pure) ---------------------------------------------------------------------------

/// What `mic_tips.json` remembers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Record {
    pub bluetooth: Count,
    pub clipped: Count,
    pub quiet: Count,
    pub noisy: Count,
    /// When a mic tip last showed (unix ms).
    pub last_shown_at: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Count {
    pub shown: u32,
    pub last_at: u64,
}

impl Record {
    fn of(&self, kind: Kind) -> Count {
        match kind {
            Kind::Bluetooth => self.bluetooth,
            Kind::Clipped => self.clipped,
            Kind::Quiet => self.quiet,
            Kind::Noisy => self.noisy,
        }
    }

    fn of_mut(&mut self, kind: Kind) -> &mut Count {
        match kind {
            Kind::Bluetooth => &mut self.bluetooth,
            Kind::Clipped => &mut self.clipped,
            Kind::Quiet => &mut self.quiet,
            Kind::Noisy => &mut self.noisy,
        }
    }
}

/// May a `kind` tip show at `now` (unix ms)?
pub fn eligible(rec: &Record, kind: Kind, now: u64) -> bool {
    let c = rec.of(kind);
    // (A clock set back waits rather than repeating.)
    let waited = |at: u64, gap: u64| at == 0 || (now >= at && now - at >= gap);
    c.shown < MAX_SHOWS && waited(rec.last_shown_at, MIN_GAP_MS) && waited(c.last_at, SAME_GAP_MS)
}

/// `rec` after a `kind` tip showed at `now`.
pub fn shown(mut rec: Record, kind: Kind, now: u64) -> Record {
    let c = rec.of_mut(kind);
    c.shown += 1;
    c.last_at = now;
    rec.last_shown_at = now;
    rec
}

// ---- mic_tips.json ------------------------------------------------------------------------------

static RECORD: Mutex<Option<Record>> = Mutex::new(None);

fn record_path() -> std::path::PathBuf {
    crate::config::data_dir().join("mic_tips.json")
}

/// The record, read the first time (`saving::Saver::loaded`: set aside if
/// it isn't one; one Yap couldn't read is never written over, and read
/// again here until it reads, what changed meanwhile kept with it). Unit
/// tests never touch the data folder.
fn loaded(slot: &mut Option<Record>) -> &mut Record {
    if cfg!(test) {
        return slot.get_or_insert_with(Record::default);
    }
    SAVER.loaded(slot, &record_path(), true)
}

fn update_record(f: impl FnOnce(Record) -> Record) {
    let mut guard = RECORD.lock().unwrap_or_else(|p| p.into_inner());
    let rec = f(loaded(&mut guard).clone());
    if !cfg!(test) {
        let _ = SAVER.write_json(&record_path(), &rec, true);
    }
    *guard = Some(rec);
}

/// mic_tips.json's saves (`saving.rs`): only logged (a tip shown once more
/// is all a lost save costs).
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::quiet("micTips", "your tips' progress", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    let mut guard = RECORD.lock().unwrap_or_else(|p| p.into_inner());
    if guard.is_some() {
        let _ = SAVER.write_json(&record_path(), &*loaded(&mut guard), true);
    }
    drop(guard);
    SAVER.check()
}

// ---- after a dictation ------------------------------------------------------------------------------

/// Test mode: tips are judged only once a spec asks (`e2e_mic_tips`).
#[cfg(debug_assertions)]
static E2E_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn judging() -> bool {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        return E2E_ON.load(std::sync::atomic::Ordering::SeqCst);
    }
    true
}

/// A dictation just stopped: its audio and microphone, judged now (cheap),
/// told once it's done (transcribed and pasted: never mid-dictation),
/// within a minute. Off the pipeline's locks: a thread of its own.
pub fn after_dictation(app: &AppHandle, audio: &[f32], mic: Option<MicInfo>, order: Vec<crate::dictation::mics::MicChoice>) {
    if !judging() || crate::notices::notice::tips_off() {
        return;
    }
    let signal = measure(audio);
    let trouble = signal.as_ref().and_then(trouble);
    #[cfg(debug_assertions)]
    if let Ok(mut last) = E2E_LAST.lock() {
        *last = Some(serde_json::json!({
            "voiceDb": signal.map(|s| s.voice_db),
            "floorDb": signal.map(|s| s.floor_db),
            "clipped": signal.map(|s| s.clipped),
            "trouble": trouble,
            "handsFree": mic.as_ref().is_some_and(hands_free),
        }));
    }
    // No voice at all: the dead-mic watchdog's to say, not a tip.
    if signal.as_ref().is_none_or(|s| s.voice_db < SILENT_DB) {
        return;
    }
    if trouble.is_none() && !mic.as_ref().is_some_and(hands_free) {
        return;
    }
    if let Some(s) = &signal {
        tracing::info!(
            secs = s.secs,
            voice_db = s.voice_db.round(),
            floor_db = s.floor_db.round(),
            clipped = s.clipped,
            ?trouble,
            "Mic tips: a weak signal"
        );
    }
    let app = app.clone();
    let _ = std::thread::Builder::new().name("dictation-mic-tip".into()).spawn(move || {
        let started = std::time::Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(500));
        while crate::notices::notice::moment().dictating && started.elapsed() < std::time::Duration::from_secs(60) {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        let mut present = crate::dictation::mics::present().unwrap_or_default();
        // The laptop's lid closed: its own mic can't hear, so it's never the
        // one to try (`lid.rs`).
        if crate::dictation::lid::closed() {
            present.retain(|e| !e.built_in);
        }
        let other = other_mic(&present, &order, mic.as_ref().and_then(|m| m.endpoint.as_deref()));
        let Some(tip) = pick(trouble, mic.as_ref(), other.as_ref()) else { return };
        offer(&app, tip);
    });
}

/// Show `tip` if it's due: the main window's toast while the person looks
/// at it, else a Yap bar card; nowhere else (a tip can go unsaid). Counted
/// only when shown.
fn offer(app: &AppHandle, tip: Tip) {
    let now = crate::notices::notice::now_ms();
    if crate::notices::notice::tips_off() {
        return;
    }
    // Never over a meeting (the main window's toast doesn't wait one out,
    // and a bar card would wait unseen and be dropped): not shown, not
    // counted.
    if crate::notices::notice::moment().meeting {
        tracing::debug!(kind = ?tip.kind, "Mic tips: a meeting is recording; not now");
        return;
    }
    let rec = loaded(&mut RECORD.lock().unwrap_or_else(|p| p.into_inner())).clone();
    if !eligible(&rec, tip.kind, now) {
        tracing::debug!(kind = ?tip.kind, "Mic tips: not now");
        return;
    }
    let place = crate::notices::notice::place_tip(app, KIND);
    let shown_now = match place {
        crate::notices::notice::Place::Window => app.emit(EVENT, &tip).is_ok(),
        crate::notices::notice::Place::Bar => {
            let card = crate::bar::Card {
                id: KIND.into(),
                icon: "mic",
                title: tip.title.clone(),
                body: tip.body.clone(),
                primary: Some(crate::bar::CardAction::new(tip.action, tip.label)),
                timeout_ms: Some(CARD_MS),
                ..Default::default()
            };
            crate::bar::show_card(app, card, Box::new(crate::notices::alert::act))
        }
        _ => false,
    };
    // Not shown after all: the day's count back.
    if crate::notices::notice::tip_shown(KIND, place, shown_now) {
        tracing::info!(kind = ?tip.kind, window = place == crate::notices::notice::Place::Window, "Mic tips: shown");
        update_record(|r| shown(r, tip.kind, now));
    } else {
        tracing::debug!(kind = ?tip.kind, "Mic tips: nowhere to show it");
    }
}

/// Test mode: the last dictation's judgement (`e2e_mic_tips`).
#[cfg(debug_assertions)]
static E2E_LAST: Mutex<Option<serde_json::Value>> = Mutex::new(None);

/// Test mode only: judge dictations' signals (`on`), with the counts
/// forgotten so every tip is due. Returns the last dictation's judgement
/// (its voice and room in dBFS, the clipped share, the trouble, whether its
/// mic was a headset in hands-free mode), for the robot's real speech.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_mic_tips(on: Option<bool>) -> Result<Option<serde_json::Value>, String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    if let Some(on) = on {
        E2E_ON.store(on, std::sync::atomic::Ordering::SeqCst);
        update_record(|_| Record::default());
    }
    Ok(E2E_LAST.lock().ok().and_then(|l| l.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_790_000_000_000;

    /// `secs` of 16 kHz audio: a voice at `voice` (peak) in syllables —
    /// 240 ms on, 160 ms off — over a room at `room` (peak, steady hiss).
    fn speech(secs: f32, voice: f32, room: f32) -> Vec<f32> {
        let n = (secs * 16_000.0) as usize;
        let mut seed = 12345u32;
        (0..n)
            .map(|i| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let hiss = ((seed >> 9) as f32 / (1u32 << 23) as f32 - 0.5) * 2.0 * room;
                let on = i % 6_400 < 3_840;
                let tone = if on { voice * (i as f32 * 2.0 * std::f32::consts::PI * 220.0 / 16_000.0).sin() } else { 0.0 };
                (tone + hiss).clamp(-1.0, 1.0)
            })
            .collect()
    }

    fn judge(samples: &[f32]) -> Option<Trouble> {
        measure(samples).as_ref().and_then(trouble)
    }

    #[test]
    fn a_clear_dictation_gets_no_tip() {
        // Speech peaking around -12 dBFS in a quiet room.
        assert_eq!(judge(&speech(4.0, 0.25, 0.001)), None);
        // A whispered word in a quiet room is still well above it.
        assert_eq!(judge(&speech(4.0, 0.03, 0.0005)), None);
    }

    #[test]
    fn a_quiet_noisy_or_clipped_signal_is_weak() {
        // Far from the mic: -50 dBFS.
        assert_eq!(judge(&speech(4.0, 0.004, 0.0002)), Some(Trouble::Quiet));
        // A loud room around a normal voice.
        assert_eq!(judge(&speech(4.0, 0.05, 0.06)), Some(Trouble::Noisy));
        // The level turned up so far it hits full scale.
        assert_eq!(judge(&speech(4.0, 1.4, 0.001)), Some(Trouble::Clipped));
    }

    #[test]
    fn a_clear_voice_in_a_long_silence_is_not_quiet() {
        // A spoken "snip": a second of clear speech, then the box drawn and
        // the question waited for in a quiet room (5 % speech).
        let mut audio = speech(1.0, 0.25, 0.001);
        audio.extend(speech(19.0, 0.0, 0.001));
        let s = measure(&audio).unwrap();
        assert!(s.voice_db > -20.0, "{s:?}");
        assert_eq!(trouble(&s), None);
        // The same half second, far from the mic, still is.
        let mut far = speech(1.0, 0.004, 0.0002);
        far.extend(speech(19.0, 0.0, 0.0002));
        assert_eq!(judge(&far), Some(Trouble::Quiet));
    }

    #[test]
    fn short_or_silent_dictations_are_never_judged() {
        assert_eq!(measure(&speech(1.0, 0.004, 0.0)), None);
        assert_eq!(judge(&vec![0.0; 64_000]), None, "digital silence is the watchdog's");
    }

    fn mic(bluetooth: bool, rate: u32) -> MicInfo {
        MicInfo { name: "Headset (Contoso Buds Hands-Free AG Audio)".into(), endpoint: Some("bt".into()), bluetooth, rate }
    }

    #[test]
    fn a_bluetooth_headset_in_hands_free_mode_gets_its_own_advice() {
        let pc = Other { name: "Microphone Array (Northwind Audio)".into(), built_in: true };
        let tip = pick(Some(Trouble::Quiet), Some(&mic(true, 16_000)), Some(&pc)).unwrap();
        assert_eq!(tip.kind, Kind::Bluetooth);
        assert_eq!(tip.body, "Bluetooth headsets sound worse while their mic is on: try the PC's mic.");
        assert_eq!(tip.title, TITLE);
        // Even when the signal was fine (it's always the call profile's).
        assert_eq!(pick(None, Some(&mic(true, 8_000)), Some(&pc)).unwrap().kind, Kind::Bluetooth);
        // An LE Audio mic at 48 kHz isn't in hands-free mode; a wired one never is.
        assert_eq!(pick(None, Some(&mic(true, 48_000)), Some(&pc)), None);
        assert_eq!(pick(None, Some(&mic(false, 16_000)), Some(&pc)), None);
        // Nothing else to try: the signal's own tip, if any.
        assert_eq!(pick(Some(Trouble::Quiet), Some(&mic(true, 16_000)), None).unwrap().kind, Kind::Quiet);
        // A USB mic to try is named.
        let usb = Other { name: "Microphone (Fabrikam USB Mic)".into(), built_in: false };
        assert!(pick(Some(Trouble::Noisy), Some(&mic(true, 16_000)), Some(&usb)).unwrap().body.contains("try Fabrikam USB Mic."));
    }

    #[test]
    fn each_trouble_has_one_concrete_tip() {
        let clipped = pick(Some(Trouble::Clipped), None, None).unwrap();
        assert_eq!((clipped.kind, clipped.action), (Kind::Clipped, crate::notices::alert::OPEN_SOUND_SETTINGS));
        assert!(clipped.body.contains("level down"));
        let quiet = pick(Some(Trouble::Quiet), None, None).unwrap();
        assert!(quiet.body.contains("Move closer"));
        let usb = Other { name: "Microphone (Fabrikam USB Mic)".into(), built_in: false };
        let noisy = pick(Some(Trouble::Noisy), Some(&mic(false, 48_000)), Some(&usb)).unwrap();
        assert_eq!(noisy.action, crate::notices::alert::OPEN_MIC_ORDER);
        assert!(noisy.body.contains("Try Fabrikam USB Mic"));
        assert!(pick(Some(Trouble::Noisy), None, None).unwrap().body.contains("Move closer"));
        assert_eq!(pick(None, None, None), None);
    }

    #[test]
    fn the_other_mic_is_the_pcs_own_first_never_bluetooth() {
        use crate::dictation::mics::Endpoint;
        let ep = |id: &str, bluetooth, built_in| Endpoint { id: id.into(), name: format!("Microphone ({id})"), bluetooth, built_in, ..Default::default() };
        let present = vec![ep("bt", true, false), ep("usb", false, false), ep("array", false, true)];
        assert_eq!(other_mic(&present, &[], Some("bt")).unwrap().name, "Microphone (array)");
        assert_eq!(other_mic(&present, &[], Some("array")).unwrap().name, "Microphone (usb)");
        assert_eq!(other_mic(&present[..1], &[], Some("bt")), None);
    }

    #[test]
    fn tips_are_rare() {
        let mut r = Record::default();
        assert!(eligible(&r, Kind::Quiet, T0));
        r = shown(r, Kind::Quiet, T0);
        // Not another mic tip the same day…
        assert!(!eligible(&r, Kind::Noisy, T0 + MIN_GAP_MS - 1));
        assert!(eligible(&r, Kind::Noisy, T0 + MIN_GAP_MS));
        // …nor the same one within a week…
        assert!(!eligible(&r, Kind::Quiet, T0 + MIN_GAP_MS));
        let mut t = T0;
        for _ in 1..MAX_SHOWS {
            t += SAME_GAP_MS;
            assert!(eligible(&r, Kind::Quiet, t));
            r = shown(r, Kind::Quiet, t);
        }
        // …and three times at most.
        assert!(!eligible(&r, Kind::Quiet, t + 365 * 24 * 3_600_000));
        assert!(eligible(&r, Kind::Bluetooth, t + MIN_GAP_MS));
        // A clock set back waits rather than repeating.
        assert!(!eligible(&shown(Record::default(), Kind::Quiet, T0), Kind::Noisy, T0 - 1_000));
        // An older file without a kind reads as none shown.
        let old: Record = serde_json::from_str(r#"{"quiet":{"shown":1}}"#).unwrap();
        assert_eq!(old.quiet.shown, 1);
        assert_eq!(old.bluetooth.shown, 0);
    }
}

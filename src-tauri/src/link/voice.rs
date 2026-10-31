//! **Yap's voice on the phone** (Yap Link): an answer is said by Yap's own
//! speaker (`voice/tts/speaker.rs`: the voice Settings picked, a sentence
//! made while the one before plays, a cloud voice's local stand-in, one
//! voice at a time, never over a dictation or a meeting) with its output
//! here instead of this PC's speakers ([`opener`]): each sentence's audio,
//! as it's made, is brought to mono [`wire::DOWN_RATE`] and sent down the
//! socket as `0x02` frames between `speak_start {rate}` and `speak_end`.
//! Nothing plays on the PC.
//!
//! The speaker waits for the phone to have played it ([`PhonePlayback::done`]:
//! the audio's length on a clock from its first frame), so its "speaking"
//! lasts as long as the phone's.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::link::wire::{self, Out, Phase};
use crate::voice::tts::{Opened, Opener, Pcm, Playback};

/// What a session's writer sends (`session.rs`).
pub enum Wire {
    Text(String),
    /// A down frame of the voice of turn `turn` (dropped if that turn was
    /// interrupted meanwhile).
    Audio { turn: u64, bytes: Vec<u8> },
    /// Close the socket.
    Close,
}

/// The session's way out, from any thread.
pub type Tx = tokio::sync::mpsc::UnboundedSender<Wire>;

/// Say `out` to the phone.
pub fn say(tx: &Tx, out: Out) {
    let _ = tx.send(Wire::Text(out.json()));
}

/// At most this much audio a frame (half a second): a long sentence goes in
/// a few, so an interrupt drops what hasn't left.
const FRAME_SAMPLES: usize = wire::DOWN_RATE as usize / 2;

/// One answer's voice on the phone.
pub struct PhonePlayback {
    tx: Tx,
    /// The session's turn this voice is for, and the session's latest turn.
    turn: u64,
    latest: Arc<std::sync::atomic::AtomicU64>,
    /// `speak_start` has gone; this voice has ended.
    started: AtomicBool,
    ended: AtomicBool,
    stopped: AtomicBool,
    seq: AtomicU32,
    /// The phone's clock: when its first frame went, and how much audio.
    clock: Mutex<(Option<Instant>, Duration)>,
    /// The turn's "a voice is going", and "its checked answer is in".
    speaking: Arc<AtomicBool>,
    answered: Arc<AtomicBool>,
}

impl PhonePlayback {
    fn live(&self) -> bool {
        !self.stopped.load(Ordering::SeqCst) && self.latest.load(Ordering::SeqCst) == self.turn
    }

    /// This voice is over (once): `speak_end` if it started, and — while it's
    /// still the latest turn — idle, or thinking when the answer isn't in yet
    /// (its first words were taken back before a tool call).
    pub fn end(&self) {
        if self.ended.swap(true, Ordering::SeqCst) {
            return;
        }
        if self.started.load(Ordering::SeqCst) {
            say(&self.tx, Out::SpeakEnd);
        }
        self.speaking.store(false, Ordering::SeqCst);
        if self.latest.load(Ordering::SeqCst) == self.turn {
            let phase = if self.answered.load(Ordering::SeqCst) { Phase::Idle } else { Phase::Thinking };
            say(&self.tx, Out::State { phase });
        }
    }
}

impl Playback for PhonePlayback {
    fn append(&self, pcm: Pcm) {
        if !self.live() {
            return;
        }
        if !self.started.swap(true, Ordering::SeqCst) {
            self.speaking.store(true, Ordering::SeqCst);
            say(&self.tx, Out::State { phase: Phase::Speaking });
            say(&self.tx, Out::SpeakStart { rate: wire::DOWN_RATE });
        }
        let mono = to_mono(&pcm);
        let samples = crate::dictation::pipeline::resample_linear(&mono, pcm.sample_rate.max(1), wire::DOWN_RATE);
        let length = Duration::from_secs_f64(samples.len() as f64 / f64::from(wire::DOWN_RATE));
        {
            let mut c = self.clock.lock().unwrap_or_else(|p| p.into_inner());
            c.0.get_or_insert_with(Instant::now);
            c.1 += length;
        }
        for chunk in wire::to_i16(&samples).chunks(FRAME_SAMPLES) {
            let seq = self.seq.fetch_add(1, Ordering::SeqCst);
            let _ = self.tx.send(Wire::Audio { turn: self.turn, bytes: wire::encode(wire::DOWN, seq, chunk) });
        }
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.end();
    }

    /// Played to the end on the phone (its clock), or stopped.
    fn done(&self) -> bool {
        if !self.live() {
            return true;
        }
        let c = self.clock.lock().unwrap_or_else(|p| p.into_inner());
        match c.0 {
            Some(from) => from.elapsed() >= c.1,
            None => true,
        }
    }
}

/// Ends the voice on the phone when the speaker lets go of its output
/// (played out, stopped or replaced: `speaker.rs` drops `Opened::keep`).
struct EndGuard(Arc<PhonePlayback>);

impl Drop for EndGuard {
    fn drop(&mut self) {
        self.0.end();
    }
}

/// Mono samples of `pcm` (its channels averaged).
fn to_mono(pcm: &Pcm) -> Vec<f32> {
    let ch = usize::from(pcm.channels.max(1));
    if ch == 1 {
        return pcm.samples.clone();
    }
    pcm.samples.chunks(ch).map(|f| f.iter().sum::<f32>() / f.len() as f32).collect()
}

/// **The output for turn `turn`'s voice**: the phone, through `tx`. `latest`:
/// the session's latest turn (an interrupt moves it on, and this voice goes
/// quiet at once); `speaking`, `answered`: the turn's own flags.
pub fn opener(
    tx: Tx,
    turn: u64,
    latest: Arc<std::sync::atomic::AtomicU64>,
    speaking: Arc<AtomicBool>,
    answered: Arc<AtomicBool>,
) -> (Opener, Arc<PhonePlayback>) {
    let play = Arc::new(PhonePlayback {
        tx,
        turn,
        latest,
        started: AtomicBool::new(false),
        ended: AtomicBool::new(false),
        stopped: AtomicBool::new(false),
        seq: AtomicU32::new(0),
        clock: Mutex::new((None, Duration::ZERO)),
        speaking,
        answered,
    });
    let given = Arc::clone(&play);
    let open: Opener = Box::new(move || {
        let keep: Box<dyn std::any::Any> = Box::new(EndGuard(Arc::clone(&given)));
        Ok(Opened { play: given as Arc<dyn Playback>, keep: Some(keep) })
    });
    (open, play)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rig() -> (tokio::sync::mpsc::UnboundedReceiver<Wire>, Arc<PhonePlayback>, Arc<std::sync::atomic::AtomicU64>, Opener) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let latest = Arc::new(std::sync::atomic::AtomicU64::new(3));
        let (open, play) = opener(tx, 3, Arc::clone(&latest), Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(true)));
        (rx, play, latest, open)
    }

    fn texts(rx: &mut tokio::sync::mpsc::UnboundedReceiver<Wire>) -> (Vec<String>, Vec<Vec<u8>>) {
        let (mut t, mut a) = (Vec::new(), Vec::new());
        while let Ok(m) = rx.try_recv() {
            match m {
                Wire::Text(s) => t.push(s),
                Wire::Audio { bytes, .. } => a.push(bytes),
                Wire::Close => {}
            }
        }
        (t, a)
    }

    #[test]
    fn a_sentence_goes_down_as_24khz_mono_frames_between_start_and_end() {
        let (mut rx, play, _, open) = rig();
        let opened = open().unwrap();
        // A second of 8 kHz stereo: 24,000 mono samples at 24 kHz, in two frames.
        opened.play.append(Pcm { sample_rate: 8_000, channels: 2, samples: vec![0.5; 16_000] });
        let (t, a) = texts(&mut rx);
        assert_eq!(t, [r#"{"type":"state","phase":"speaking"}"#, r#"{"type":"speak_start","rate":24000}"#]);
        assert_eq!(a.len(), 2);
        let frames: Vec<wire::Frame> = a.iter().map(|b| wire::decode(b).unwrap()).collect();
        assert!(frames.iter().all(|f| f.kind == wire::DOWN));
        assert_eq!(frames.iter().map(|f| f.seq).collect::<Vec<_>>(), [0, 1]);
        let total: usize = frames.iter().map(|f| f.samples.len()).sum();
        assert!((23_990..=24_010).contains(&total), "{total}");
        assert!(frames[0].samples[100] > 16_000, "the level kept");
        assert!(!opened.play.done(), "the phone is still playing it");
        // The speaker lets go: speak_end, then idle.
        drop(opened.keep);
        let (t, _) = texts(&mut rx);
        assert_eq!(t, [r#"{"type":"speak_end"}"#, r#"{"type":"state","phase":"idle"}"#]);
        drop(play);
    }

    #[test]
    fn an_interrupted_voice_sends_nothing_more_and_ends_once() {
        let (mut rx, play, latest, open) = rig();
        let opened = open().unwrap();
        opened.play.append(Pcm { sample_rate: 24_000, channels: 1, samples: vec![0.1; 2_400] });
        let _ = texts(&mut rx);
        // The phone interrupted: a newer turn.
        latest.store(4, Ordering::SeqCst);
        opened.play.append(Pcm { sample_rate: 24_000, channels: 1, samples: vec![0.1; 2_400] });
        assert!(opened.play.done());
        opened.play.stop();
        drop(opened.keep);
        let (t, a) = texts(&mut rx);
        assert!(a.is_empty(), "no audio after the interrupt");
        assert_eq!(t, [r#"{"type":"speak_end"}"#], "once, and no idle over the newer turn");
        drop(play);
    }
}

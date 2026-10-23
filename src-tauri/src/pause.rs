//! **"Stop listening when I pause"** (Settings → Voice Assistant, on by
//! default): once you've asked the Voice Assistant something, a pause ends
//! the question as if you'd pressed its shortcut again — the stop chime, the
//! transcription, the answer. Never before you've said anything (someone
//! gathering their thoughts first keeps the mic), and never while the
//! push-to-talk key is held (letting go ends it): toggle mode and
//! push-to-talk's double-tap lock only (`pipeline.rs` decides that, and acts
//! on [`Pause::verdict`]). Dictation never ends by itself.
//!
//! Pure: the recording's audio, "speak now" (the start chime) and the live
//! preview's words go in; keep listening / the pause is running out / end
//! now comes out. Everything is timed in **audio** (samples of the
//! recording), never on the clock, so a slow PC or a slow preview only makes
//! it later, never early.
//!
//! **What "speech" and "a pause" are.** The mic's own level, against a noise
//! floor the room teaches it — cheap, on every PC, with every speech model
//! and in the stub build — checked by the live preview's words when they run
//! (`partials.rs`, on by default):
//!
//! - Each 20 ms frame's loudness (RMS, dBFS). The **noise floor** follows the
//!   quiet between words down quickly and creeps up slowly
//!   ([`FLOOR_RISE_DB_PER_S`]), so a fan, a hum or steady music becomes the
//!   floor within seconds, while speech — which dips between words, and
//!   between breaths — never does. A frame is **speech** at [`MARGIN_DB`]
//!   over the floor, and never quieter than [`SPEECH_MIN_DB`] (about RMS
//!   0.003: OpenWhispr's local speech gate, `helpers/localSpeechGate.js`,
//!   counts a window as speech from there). Digital silence (a cold mic, a
//!   dropout) teaches the floor nothing: one dropout must not make the room
//!   sound like talk for seconds after.
//! - A sound counts once it lasts [`ONSET_MS`] (Handy's VAD needs two 30 ms
//!   frames too): a click, a tap or a key doesn't. Nothing counts for
//!   [`SETTLE_MS`] after "speak now" — the start chime may reach the mic.
//! - **Heard**: [`HEARD_MS`] of speech. From then on **a pause** of
//!   [`PAUSE_MS`] ends the question; [`CUE_AFTER_MS`] into it the bar's
//!   capsule shows it running out ([`Verdict::Ending`]).
//! - **The live preview's words**, once a tick of it has come back: a pause
//!   ends nothing until there are words (a cough or a door isn't a
//!   question; `[BLANK_AUDIO]`, `(music)` aren't words). More words than
//!   ever, in a stretch the level called quiet, is speech the level missed
//!   (a soft voice over noise): the pause counts from there. And **no new
//!   words for [`STALL_MS`] of audio** ends it too, the level loud or not:
//!   music, a TV nobody's talking on, a noise the floor hasn't learned yet.
//!   Only after the words grew twice: one line a model makes up from music
//!   and keeps isn't a question.
//!
//! **What it can't tell**: a sound the floor first takes for talk and then
//! learns — music that starts as you start listening — can end a question
//! before you've spoken when the live preview is off, or when the model
//! makes words up from it (Whisper's "Thank you."). With the preview's
//! words (Parakeet, by default) it holds.
//!
//! **Why not a voice-activity model** (Handy runs Silero through ONNX): it's
//! a model to ship or download for the stub build and every PC, and it
//! decides "voice" — music with singing, a TV — not "you've finished". The
//! level with a learned floor never ends a question early because of noise
//! (noise can only keep it open, the shortcut still ends it), and the words
//! catch what the level can't.
//!
//! **Why 1.5 s.** None of the apps Yap borrows from ends a recording on a
//! pause: Handy's Silero VAD (threshold 0.3, 450 ms hangover, 60 ms onset,
//! `managers/audio.rs`) only trims silence out of a recording, FluidVoice's
//! level has a noise gate for its meter, and OpenWhispr stops dictation only
//! on the key — its one silence setting is the 600 ms server VAD it asks of
//! OpenAI's Realtime API (`openaiRealtimeStreaming.js`) to cut a live
//! transcript into turns, where cutting early costs nothing. 600 ms would
//! end "what's on my calendar for… Friday" at its first thinking pause. A
//! question's hesitations are mostly well under a second, and the
//! maintainer's log that asked for this sat 6 s in silence; 1.5 s ends the
//! question promptly and lets a short "um, where was I" pause through, and
//! the cue shows a second before it ends.

/// The pipeline's capture rate (16 kHz mono), in samples per millisecond.
const PER_MS: u64 = 16;
/// One frame: 20 ms.
const FRAME: usize = 320;
/// One frame, in ms.
pub const FRAME_MS: u64 = 20;

/// The quiet after your question that ends it.
pub const PAUSE_MS: u64 = 1_500;
/// How far into the pause the bar shows it running out.
pub const CUE_AFTER_MS: u64 = 500;
/// After "speak now" (the start chime), nothing counts for this long.
pub const SETTLE_MS: u64 = 400;
/// A sound this long is speech; anything shorter is a click.
pub const ONSET_MS: u64 = 60;
/// This much speech before a pause can end anything.
pub const HEARD_MS: u64 = 300;
/// No new words in the live preview for this much audio ends it too.
pub const STALL_MS: u64 = 4_000;

/// Speech is this far over the noise floor…
pub const MARGIN_DB: f32 = 10.0;
/// …and never quieter than this (RMS ≈ 0.003).
pub const SPEECH_MIN_DB: f32 = -50.0;
/// The floor creeps up this fast under a sound that stays.
pub const FLOOR_RISE_DB_PER_S: f32 = 4.0;
/// The floor follows a quieter frame down by this share of the way.
const FLOOR_FALL: f32 = 0.25;
/// Quieter than this is digital silence: a cold mic, a dropout.
const DIGITAL_SILENCE_DB: f32 = -90.0;

/// Why a question ended by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// [`PAUSE_MS`] of quiet after speech.
    Pause,
    /// [`STALL_MS`] of audio without a new word.
    NoNewWords,
}

/// What the pipeline should do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Keep listening.
    Listen,
    /// A pause is running: it ends the question in `left_ms` (the bar's
    /// cue).
    Ending { left_ms: u64 },
    /// End the question now.
    End(Why),
}

/// One recording's pause watch (see the module docs).
#[derive(Debug, Default)]
pub struct Pause {
    /// The audio clock: samples of the recording, at the end of the last
    /// whole frame.
    at: u64,
    /// The frame being gathered: the sum of its squares, and how many.
    sum_sq: f64,
    n: usize,
    /// The noise floor (dBFS), once a frame has taught it something.
    floor: Option<f32>,
    /// Speech counts from here on ("speak now" + [`SETTLE_MS`]).
    counts_from: Option<u64>,
    /// The sound going on now: how long (ms).
    run_ms: u64,
    /// Speech heard so far (ms).
    heard_ms: u64,
    /// Where the last speech was (the end of its frame).
    last_speech: Option<u64>,
    /// The live preview: ticks come back…
    ticks: u32,
    /// …the most words it has shown…
    most_words: usize,
    /// …how often that went up…
    growths: u32,
    /// …where the audio it had read ended the last time it did…
    grew_at: u64,
    /// …and the furthest it has read.
    covered: u64,
}

impl Pause {
    /// A watch whose clock starts at `at` (the recording's buffer already
    /// holds that many samples: the pre-roll), so the live preview's
    /// positions in the buffer and this clock agree.
    pub fn new(at: u64) -> Self {
        Pause { at, ..Pause::default() }
    }

    /// "Speak now": the start chime played (or would have), the mic is on.
    /// Speech counts from [`SETTLE_MS`] after this. Only the first call.
    pub fn speak_now(&mut self) {
        if self.counts_from.is_none() {
            self.counts_from = Some(self.at + self.n as u64 + SETTLE_MS * PER_MS);
        }
    }

    /// The recording's audio as it arrives (16 kHz mono).
    pub fn audio(&mut self, samples: &[f32]) {
        for &s in samples {
            self.sum_sq += f64::from(s) * f64::from(s);
            self.n += 1;
            if self.n == FRAME {
                let rms = (self.sum_sq / FRAME as f64).sqrt();
                self.sum_sq = 0.0;
                self.n = 0;
                let db = if rms > 0.0 { (20.0 * rms.log10()) as f32 } else { f32::NEG_INFINITY };
                self.frame(db);
            }
        }
    }

    /// One whole frame, `db` loud.
    fn frame(&mut self, db: f32) {
        self.at += FRAME as u64;
        let digital_silence = db <= DIGITAL_SILENCE_DB;
        // Against the floor as it was: a loud frame mustn't lift the bar
        // it's measured against.
        let speech = !digital_silence && db >= SPEECH_MIN_DB && self.floor.is_none_or(|f| db >= f + MARGIN_DB);
        if !digital_silence {
            self.floor = Some(match self.floor {
                None => db,
                Some(f) if db < f => f + (db - f) * FLOOR_FALL,
                Some(f) => f + (db - f).min(FLOOR_RISE_DB_PER_S * FRAME_MS as f32 / 1000.0),
            });
        }
        let counts = speech && self.counts_from.is_some_and(|from| self.at > from);
        if !counts {
            self.run_ms = 0;
            return;
        }
        self.run_ms += FRAME_MS;
        if self.run_ms >= ONSET_MS {
            // The frames that made it a sound count too.
            self.heard_ms += if self.run_ms - FRAME_MS < ONSET_MS { self.run_ms } else { FRAME_MS };
            self.last_speech = Some(self.at);
        }
    }

    /// A tick of the live preview came back: `text` is what it shows for the
    /// recording's first `covered` samples.
    pub fn words(&mut self, covered: u64, text: &str) {
        let before = self.covered;
        self.covered = self.covered.max(covered);
        self.ticks += 1;
        let n = word_count(text);
        if n <= self.most_words {
            return;
        }
        self.most_words = n;
        self.growths += 1;
        self.grew_at = covered;
        // New words where the level heard nothing since the last tick: a
        // voice it missed. The pause counts from the end of what was read.
        if self.counts_from.is_some() && self.last_speech.is_none_or(|s| s <= before) {
            self.last_speech = Some(self.last_speech.map_or(covered, |s| s.max(covered)));
        }
    }

    /// Where the question is (see [`Verdict`]).
    pub fn verdict(&self) -> Verdict {
        if self.heard_ms < HEARD_MS {
            return Verdict::Listen;
        }
        // The live preview runs and has no words: nothing to ask yet.
        if self.ticks > 0 && self.most_words == 0 {
            return Verdict::Listen;
        }
        if self.growths >= 2 && self.covered.saturating_sub(self.grew_at) >= STALL_MS * PER_MS {
            return Verdict::End(Why::NoNewWords);
        }
        let quiet_ms = self.last_speech.map_or(0, |s| self.at.saturating_sub(s) / PER_MS);
        if quiet_ms >= PAUSE_MS {
            Verdict::End(Why::Pause)
        } else if quiet_ms >= CUE_AFTER_MS {
            Verdict::Ending { left_ms: PAUSE_MS - quiet_ms }
        } else {
            Verdict::Listen
        }
    }

    /// Speech heard so far (ms), for the log.
    pub fn heard_ms(&self) -> u64 {
        self.heard_ms
    }
}

/// Words in a live preview's line: tokens with a letter or a digit, leaving
/// out what a model writes in brackets for a sound (`[BLANK_AUDIO]`,
/// `(music)`, `[Music]`) and `♪`.
pub fn word_count(text: &str) -> usize {
    let mut depth = 0usize;
    let mut kept = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => kept.push(c),
            _ => {}
        }
    }
    kept.split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The room: a quiet mic's hiss.
    const ROOM: f32 = -62.0;
    /// Someone talking at a headset or desk mic.
    const TALK: f32 = -24.0;

    /// Drives a [`Pause`] in frames of a level, the clock in ms since
    /// "speak now" (fed from the start, as the pipeline does).
    struct Rig {
        p: Pause,
    }

    impl Rig {
        /// A recording whose mic is on: "speak now" at once, then the room
        /// for the settle time.
        fn new() -> Self {
            let mut p = Pause::new(0);
            p.speak_now();
            let mut r = Rig { p };
            r.level(ROOM, SETTLE_MS);
            r
        }
        /// `ms` of frames at `db`.
        fn level(&mut self, db: f32, ms: u64) -> &mut Self {
            for _ in 0..ms / FRAME_MS {
                self.p.frame(db);
            }
            self
        }
        fn talk(&mut self, ms: u64) -> &mut Self {
            self.level(TALK, ms)
        }
        fn quiet(&mut self, ms: u64) -> &mut Self {
            self.level(ROOM, ms)
        }
        /// Now, in ms of audio.
        fn now(&self) -> u64 {
            self.p.at / PER_MS
        }
        /// Feeds `ms` of the room a frame at a time; the ms (from now) at
        /// which the verdict first said end, if it did.
        fn quiet_until_end(&mut self, ms: u64) -> Option<(u64, Why)> {
            for i in 1..=ms / FRAME_MS {
                self.p.frame(ROOM);
                if let Verdict::End(why) = self.p.verdict() {
                    return Some((i * FRAME_MS, why));
                }
            }
            None
        }
        /// A tick of the live preview, read up to now.
        fn words(&mut self, text: &str) -> &mut Self {
            let at = self.p.at;
            self.p.words(at, text);
            self
        }
    }

    /// `ms` of a 440 Hz tone at `db` RMS, as the mic would deliver it.
    fn tone(db: f32, ms: u64) -> Vec<f32> {
        let amp = 10f32.powf(db / 20.0) * std::f32::consts::SQRT_2;
        (0..ms * PER_MS).map(|i| amp * (i as f32 * std::f32::consts::TAU * 440.0 / 16_000.0).sin()).collect()
    }

    #[test]
    fn silence_alone_never_ends_it() {
        // Gathering your thoughts: a minute of the room, nothing said.
        let mut r = Rig::new();
        assert_eq!(r.quiet_until_end(60_000), None);
        assert_eq!(r.p.verdict(), Verdict::Listen);
    }

    #[test]
    fn a_pause_after_the_question_ends_it() {
        let mut r = Rig::new();
        r.quiet(800).talk(2_500);
        assert_eq!(r.p.verdict(), Verdict::Listen);
        // Ends the frame the pause is whole.
        assert_eq!(r.quiet_until_end(5_000), Some((PAUSE_MS, Why::Pause)));
    }

    #[test]
    fn the_pause_runs_out_on_the_bar_first() {
        let mut r = Rig::new();
        r.talk(1_000).quiet(CUE_AFTER_MS - FRAME_MS);
        assert_eq!(r.p.verdict(), Verdict::Listen);
        r.quiet(FRAME_MS);
        assert_eq!(r.p.verdict(), Verdict::Ending { left_ms: PAUSE_MS - CUE_AFTER_MS });
        r.quiet(600);
        assert_eq!(r.p.verdict(), Verdict::Ending { left_ms: PAUSE_MS - CUE_AFTER_MS - 600 });
        // Talking again: the cue goes, and a new pause starts from scratch.
        r.talk(400);
        assert_eq!(r.p.verdict(), Verdict::Listen);
        assert_eq!(r.quiet_until_end(5_000), Some((PAUSE_MS, Why::Pause)));
    }

    #[test]
    fn a_thinking_pause_mid_sentence_doesnt_end_it() {
        // "What's on my calendar for…" (a second to think) "…Friday?"
        let mut r = Rig::new();
        r.talk(1_400).quiet(1_200).talk(900);
        assert_eq!(r.p.verdict(), Verdict::Listen);
        r.talk(300).quiet(PAUSE_MS - 100).talk(500);
        assert_eq!(r.p.verdict(), Verdict::Listen);
        assert_eq!(r.quiet_until_end(5_000), Some((PAUSE_MS, Why::Pause)));
    }

    #[test]
    fn clicks_and_taps_neither_start_nor_hold_the_question() {
        // A keyboard's clicks: loud, but each shorter than a sound.
        let mut r = Rig::new();
        for _ in 0..50 {
            r.level(-20.0, ONSET_MS - FRAME_MS).quiet(200);
        }
        assert_eq!(r.p.heard_ms(), 0);
        assert_eq!(r.p.verdict(), Verdict::Listen);
        // After a question, clicks in the pause don't start it again…
        r.talk(1_000).quiet(700).level(-20.0, 40).quiet(200).level(-20.0, 40);
        let at = r.now();
        let ended = r.quiet_until_end(5_000).map(|(ms, _)| at + ms);
        let spoke = at - (700 + 40 + 200 + 40);
        assert_eq!(ended, Some(spoke + PAUSE_MS), "counted from the last speech");
        // …but a word does.
        let mut r = Rig::new();
        r.talk(1_000).quiet(1_000).talk(ONSET_MS);
        assert_eq!(r.quiet_until_end(5_000), Some((PAUSE_MS, Why::Pause)));
    }

    #[test]
    fn a_cough_alone_isnt_enough_speech() {
        let mut r = Rig::new();
        r.talk(HEARD_MS - FRAME_MS);
        assert_eq!(r.quiet_until_end(10_000), None);
        // A few more words make it a question.
        r.talk(FRAME_MS * 3);
        assert!(r.quiet_until_end(10_000).is_some());
    }

    #[test]
    fn the_start_chime_isnt_speech() {
        // Speaking already as "speak now" comes, the chime loud on the mic:
        // nothing counts for the settle time.
        let mut p = Pause::new(0);
        p.speak_now();
        let mut r = Rig { p };
        r.level(-18.0, SETTLE_MS);
        assert_eq!(r.p.heard_ms(), 0);
        assert_eq!(r.quiet_until_end(10_000), None);
    }

    #[test]
    fn nothing_counts_before_speak_now() {
        // A cold mic: its first audio before the chime said "speak now".
        let mut r = Rig { p: Pause::new(0) };
        r.talk(2_000);
        assert_eq!(r.p.heard_ms(), 0);
        r.p.speak_now();
        r.quiet(SETTLE_MS).talk(1_000);
        assert_eq!(r.p.heard_ms(), 1_000);
    }

    #[test]
    fn a_noisy_room_is_learned_and_speech_still_heard_over_it() {
        // A fan at −38 dBFS: louder than the speech minimum, so a fixed
        // threshold would call it talk and never end.
        let mut p = Pause::new(0);
        p.speak_now();
        let mut r = Rig { p };
        r.level(-38.0, 1_000);
        assert_eq!(r.p.heard_ms(), 0, "the fan isn't speech");
        // Talking over it, then the fan alone again.
        r.level(-20.0, 1_500);
        assert!(r.p.heard_ms() >= 1_400);
        let mut ended = None;
        for i in 1..=200 {
            r.p.frame(-38.0);
            if let Verdict::End(why) = r.p.verdict() {
                ended = Some((i * FRAME_MS, why));
                break;
            }
        }
        assert_eq!(ended, Some((PAUSE_MS, Why::Pause)));
    }

    #[test]
    fn a_sound_that_starts_and_stays_holds_it_open_until_learned() {
        // Music starts the moment you stop talking: at first it's sound
        // (never an early end), then the floor learns it and the pause runs.
        let mut r = Rig::new();
        r.talk(1_500);
        let music = -36.0;
        let mut ended_after = None;
        for i in 1..=30_000 / FRAME_MS {
            r.p.frame(music);
            if matches!(r.p.verdict(), Verdict::End(_)) {
                ended_after = Some(i * FRAME_MS);
                break;
            }
        }
        let ms = ended_after.expect("the floor learns a steady sound");
        // The floor (about −56 dBFS after the talk) climbs the 10 dB to
        // within the margin of the music at 4 dB/s: ~2.5 s, then the pause.
        assert!(ms > 2_000 + PAUSE_MS && ms < 3_000 + PAUSE_MS, "{ms}");
    }

    #[test]
    fn digital_silence_teaches_the_floor_nothing() {
        // A dropout mid-question: the room after it isn't taken for talk.
        let mut r = Rig::new();
        r.talk(1_000).level(f32::NEG_INFINITY, 300);
        assert_eq!(r.quiet_until_end(5_000), Some((PAUSE_MS - 300, Why::Pause)));
    }

    #[test]
    fn it_reads_real_audio_in_frames() {
        let mut p = Pause::new(0);
        p.speak_now();
        p.audio(&tone(ROOM, SETTLE_MS));
        // A tone at speech level, in odd-sized chunks as a mic delivers it.
        let speech = tone(TALK, 1_000);
        for chunk in speech.chunks(441) {
            p.audio(chunk);
        }
        assert_eq!(p.heard_ms(), 1_000);
        assert_eq!(p.at, (SETTLE_MS + 1_000) * PER_MS);
        // Quiet: the pause runs.
        p.audio(&tone(ROOM, PAUSE_MS - FRAME_MS));
        assert!(matches!(p.verdict(), Verdict::Ending { left_ms: FRAME_MS }));
        p.audio(&tone(ROOM, FRAME_MS));
        assert_eq!(p.verdict(), Verdict::End(Why::Pause));
        // Exact zeros are digital silence, not a crash.
        let mut p = Pause::new(0);
        p.audio(&[0.0; 3 * FRAME]);
        assert_eq!(p.floor, None);
    }

    #[test]
    fn the_preview_must_have_words_before_a_pause_ends_it() {
        // A door, a cough: loud enough, long enough, but the preview heard
        // no words.
        let mut r = Rig::new();
        r.talk(600).words("").quiet(500).words("[BLANK_AUDIO]");
        assert_eq!(r.quiet_until_end(10_000), None);
        // Without the preview (off, or its model still loading), the level
        // alone decides.
        let mut r = Rig::new();
        r.talk(600);
        assert!(r.quiet_until_end(10_000).is_some());
    }

    #[test]
    fn new_words_in_a_quiet_stretch_are_speech_the_level_missed() {
        // A soft voice under noise: the level heard the start, then the
        // preview's words keep growing where the level called it quiet.
        let mut r = Rig::new();
        r.talk(800).words("what's the");
        r.quiet(500).words("what's the weather");
        r.quiet(500).words("what's the weather in");
        r.quiet(500).words("what's the weather in Atlantis");
        // 1.5 s of "quiet" so far, but the last new words were read just now.
        assert!(!matches!(r.p.verdict(), Verdict::End(_)));
        assert_eq!(r.quiet_until_end(5_000), Some((PAUSE_MS, Why::Pause)));
    }

    #[test]
    fn words_the_level_heard_dont_push_the_pause_back() {
        // The usual case: the last words' tick comes after you stopped, and
        // the level heard them: the pause counts from the speech, not the
        // tick.
        let mut r = Rig::new();
        r.talk(1_000).words("what time").talk(500).quiet(300).words("what time is it");
        assert_eq!(r.quiet_until_end(5_000), Some((PAUSE_MS - 300, Why::Pause)));
    }

    #[test]
    fn a_line_that_flickers_isnt_new_words() {
        // A model adds a word, takes it back, adds it again: only more words
        // than ever count.
        let mut r = Rig::new();
        r.talk(1_000).words("what time is it");
        r.quiet(500).words("what time is it. Thank you.");
        r.quiet(100).words("what time is it");
        r.quiet(100).words("what time is it. Thank you.");
        // Counted once, from the first time.
        let at = r.now();
        let ended = r.quiet_until_end(5_000).map(|(ms, _)| at + ms);
        assert_eq!(ended, Some(SETTLE_MS + 1_000 + 500 + PAUSE_MS));
    }

    #[test]
    fn no_new_words_for_long_ends_it_over_music() {
        // Music the floor doesn't learn (it keeps changing): the level never
        // goes quiet, but the words stopped.
        let mut r = Rig::new();
        r.talk(700).words("play the");
        r.talk(700).words("play the news");
        let stopped = r.now();
        let mut ended = None;
        for i in 0..200 {
            // Loud, then louder: never a pause.
            r.level(if i % 2 == 0 { -22.0 } else { -12.0 }, 200);
            r.words("play the news");
            if let Verdict::End(why) = r.p.verdict() {
                ended = Some((r.now() - stopped, why));
                break;
            }
        }
        assert_eq!(ended, Some((STALL_MS, Why::NoNewWords)));
    }

    /// Music: 100 ms at a time, anywhere from −40 to −10 dBFS (the same
    /// tune every run).
    fn music(r: &mut Rig, ms: u64, seed: &mut u32) {
        for _ in 0..ms / 100 {
            *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            r.level(-40.0 + (*seed >> 8) as f32 / (1u32 << 24) as f32 * 30.0, 100);
        }
    }

    #[test]
    fn music_keeps_it_listening_and_one_made_up_line_isnt_a_question() {
        // Music from the start, and a model that hears "Thank you." in it:
        // the music never sounds like a pause, and one line that never grows
        // never stalls.
        let mut r = Rig::new();
        let mut seed = 7;
        for _ in 0..60 {
            music(&mut r, 500, &mut seed);
            r.words("Thank you.");
            assert!(!matches!(r.p.verdict(), Verdict::End(_)), "ended at {} ms", r.now());
        }
        // You ask over it, then stop: no new words for a while ends it.
        r.words("Thank you. What's on my");
        music(&mut r, 500, &mut seed);
        r.words("Thank you. What's on my calendar today");
        let stopped = r.now();
        let mut ended = None;
        for _ in 0..20 {
            music(&mut r, 500, &mut seed);
            r.words("Thank you. What's on my calendar today");
            if let Verdict::End(why) = r.p.verdict() {
                ended = Some((r.now() - stopped, why));
                break;
            }
        }
        assert_eq!(ended, Some((STALL_MS, Why::NoNewWords)));
    }

    #[test]
    fn talking_on_without_new_words_yet_keeps_listening() {
        // Words grow tick after tick while you talk: never stalled.
        let mut r = Rig::new();
        let mut said = String::new();
        for w in ["so", "what", "I", "want", "to", "know", "is", "whether", "it", "rains"] {
            said.push_str(w);
            said.push(' ');
            r.talk(500).words(&said);
            assert_eq!(r.p.verdict(), Verdict::Listen);
        }
    }

    #[test]
    fn counts_words_not_sounds() {
        assert_eq!(word_count(""), 0);
        assert_eq!(word_count("  "), 0);
        assert_eq!(word_count("[BLANK_AUDIO]"), 0);
        assert_eq!(word_count("(music) ♪ ♪"), 0);
        assert_eq!(word_count("[Music] What's the weather?"), 3);
        assert_eq!(word_count("It's 3 o'clock — isn't it?"), 5);
        assert_eq!(word_count("[STT stub: received 1.5s of audio, engine=stub]"), 0);
        assert_eq!(word_count("hello [unclosed"), 1);
    }
}

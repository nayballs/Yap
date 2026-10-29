//! Dictation: from the shortcut to the words. The pipeline and its
//! gestures, the microphone and its level meter, speech-to-text and which
//! device runs it, "Taking longer than usual…", the length limit, Upload's
//! decoding, the chimes, other apps' sound while you talk, history and a
//! day of dictation audio, the stats that outlast it and word milestones,
//! saved transforms, My languages.
//! How it works: docs/architecture/dictation.md.

pub mod agent_detect;
pub mod cap;
/// `yap.exe dictation-check` (debug builds): the person's own dictations as
/// a test set for speech-to-text and cleanup.
#[cfg(debug_assertions)]
pub mod check;
pub mod coaching;
pub mod commands;
pub mod gesture;
pub mod history;
pub mod input_hook;
pub mod languages;
pub mod media;
pub mod media_pause;
pub mod meter;
pub mod mic;
pub mod mic_privacy;
pub mod mics;
pub mod mic_watch;
pub mod milestones;
pub mod mute;
pub mod partials;
pub mod pipeline;
pub mod recent_audio;
pub mod slow;
pub mod sound;
pub mod stats;
pub mod stt;
pub mod stt_speed;
pub mod transforms;

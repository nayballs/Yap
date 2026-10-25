//! Dictation: from the shortcut to the words. The pipeline and its
//! gestures, the microphone, speech-to-text and which device runs it,
//! Upload's decoding, the chimes, other apps' sound while you talk,
//! history and a day of dictation audio, saved transforms.
//! How it works: docs/architecture/dictation.md.

pub mod agent_detect;
pub mod coaching;
pub mod commands;
pub mod gesture;
pub mod history;
pub mod input_hook;
pub mod media;
pub mod media_pause;
pub mod mic;
pub mod mic_privacy;
pub mod mics;
pub mod mic_watch;
pub mod mute;
pub mod partials;
pub mod pause;
pub mod pipeline;
pub mod recent_audio;
pub mod sound;
pub mod stt;
pub mod stt_speed;
pub mod transforms;

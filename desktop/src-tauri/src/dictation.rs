//! Dictation: from the shortcut to the words. The pipeline and its
//! gestures, the microphone and its level meter, speech-to-text and which
//! device runs it, "Taking longer than usual…", the length limit, Upload's
//! decoding, Yap's sounds (in packs), other apps' sound while you talk (and
//! while Yap speaks: `other_apps`), history and a
//! day of dictation audio, the stats that outlast it and word milestones,
//! saved transforms, My languages, the mouse's side buttons set up in one
//! step (`side_buttons`), whether the rewrite shortcut's words about a
//! selection are an edit or a question (`ask_or_edit`), and when a
//! Command reads the window in front (`command_screen`).
//! How it works: docs/architecture/dictation.md.

pub mod agent_detect;
pub mod ask_or_edit;
pub mod cap;
pub mod command_screen;
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
pub mod lid;
pub mod media;
pub mod media_pause;
pub mod meter;
pub mod mic;
pub mod mic_privacy;
pub mod mic_quiz;
pub mod mic_tips;
pub mod mics;
pub mod mic_watch;
pub mod milestones;
pub mod mute;
pub mod other_apps;
pub mod partials;
pub mod pipeline;
pub mod recent_audio;
pub mod side_buttons;
pub mod slow;
pub mod sound;
pub mod stats;
pub mod stt;
pub mod stt_speed;
pub mod tips;
pub mod transforms;

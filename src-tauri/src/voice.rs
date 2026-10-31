//! Spoken replies and Read aloud: Kokoro, Windows' voices and the cloud
//! voices (ElevenLabs, and OpenAI, Gemini and Groq on the keys Yap already
//! has), the speaker queue, word timing, reading any app's selection, and
//! other apps turned down while Yap speaks (`duck`).
//! How it works: docs/architecture/spoken-replies.md.

pub mod duck;
pub mod read_aloud;
pub mod tts;

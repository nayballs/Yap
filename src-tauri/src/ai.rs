//! AI models: what every model is told about you, the OpenAI-compatible
//! client and its prompts, cleanup styles, the on-device model (llamafile,
//! GGUF headers, the graphics card), which model a call goes to, the
//! providers' live model lists, Groq usage and the model evaluations.
//! How it works: docs/architecture/ai-models.md.

pub mod about_you;
pub mod ai_model;
pub mod cleanup_styles;
pub mod eval;
pub mod gguf;
pub mod gpu;
pub mod hardware;
pub mod llm;
pub mod local_llm;
pub mod model_catalog;
pub mod usage;

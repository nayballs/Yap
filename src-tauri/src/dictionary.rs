//! The dictionary: near-miss correction, how often each word is used,
//! words learned from your corrections, names spelled the way the screen
//! shows them, and the Dictionary view's import and export files. How it
//! works: docs/architecture/dictionary.md.

pub mod dict_usage;
pub mod files;
pub mod fuzzy;
pub mod learn;
pub mod screen_terms;

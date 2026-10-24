//! The dictionary: near-miss correction, how often each word is used,
//! words learned from your corrections, and names spelled the way the
//! screen shows them. How it works: docs/architecture/dictionary.md.

pub mod dict_usage;
pub mod fuzzy;
pub mod learn;
pub mod screen_terms;

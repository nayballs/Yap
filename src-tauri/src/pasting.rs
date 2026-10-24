//! Pasting: the words into the app you're in. The verified clipboard
//! paste and the submit key, "Use what I said", apps running as
//! administrator, UI Automation reads of the field, smart spacing and
//! the kind of app. How it works: docs/architecture/pasting.md.

pub mod app_kind;
pub mod as_said;
pub mod elevated;
pub mod selection;
pub mod smart_insert;
pub mod text_injector;

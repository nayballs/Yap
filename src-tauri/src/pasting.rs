//! Pasting: the words into the app you're in. The verified clipboard
//! paste and the submit key, "Use what I said", Paste / Copy last
//! transcript on their shortcuts, apps running as administrator, the
//! modifier keys around Yap's own, long pastes in slices, UI Automation
//! reads of the field, smart spacing and the kind of app. How it works:
//! docs/architecture/pasting.md.

pub mod app_kind;
pub mod as_said;
pub mod elevated;
pub mod last;
pub mod modifiers;
pub mod read_check;
pub mod selection;
pub mod slices;
pub mod smart_insert;
pub mod text_injector;

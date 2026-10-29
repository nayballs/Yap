//! Pasting: the words into the app you're in. The verified clipboard
//! paste and the submit key, "Use what I said", Paste / Copy last
//! transcript on their shortcuts, apps running as administrator, the
//! modifier keys around Yap's own, long pastes in slices, UI Automation
//! reads of the field, smart spacing and the kind of app; in chat apps and
//! AI chat boxes, stacked messages, @mentions and file tags pasted in
//! steps, and the Send shortcut. How it works: docs/architecture/pasting.md.

pub mod app_kind;
pub mod as_said;
pub mod compose;
pub mod elevated;
pub mod file_tags;
pub mod last;
pub mod mentions;
pub mod modifiers;
pub mod read_check;
pub mod selection;
pub mod send_key;
pub mod slices;
pub mod smart_insert;
pub mod stacked;
pub mod text_injector;

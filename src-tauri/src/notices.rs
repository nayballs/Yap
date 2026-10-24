//! Notices: the one policy for what Yap says and where, one-off alerts
//! to act on, and Windows notifications.
//! How it works: docs/architecture/notices.md.

pub mod alert;
pub mod notice;
#[cfg(windows)]
pub mod win_toast;

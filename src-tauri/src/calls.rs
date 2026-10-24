//! Calls: noticing a call has started, the call apps' icons, a
//! recording's guard rails, and keeping meeting windows out of screen
//! captures. How it works: docs/architecture/call-detection-and-guards.md.

pub mod app_icons;
pub mod capture;
pub mod meeting_detect;
pub mod meeting_guard;

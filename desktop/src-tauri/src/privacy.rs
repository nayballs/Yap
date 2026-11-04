//! Privacy: network receipts for every connection Yap makes, Report a
//! problem's checked diagnostic file, Download your data and Reset app, and
//! the test that keeps your words out of the log. How it works:
//! docs/architecture/privacy-receipts.md.

pub mod diagnostics;
pub mod export;
#[cfg(test)]
pub mod log_privacy;
pub mod net;
pub mod reset;

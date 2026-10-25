//! **Meeting search and Ask across meetings** (2026-10-25): local semantic
//! search over every meeting and note, and answers that cite the moment.
//!
//! - [`chunks`]: notes cut into passages (a meeting's talk by speaker turns,
//!   notes and summaries by paragraphs), each keeping its lines.
//! - [`embedder`]: EmbeddingGemma-300M on Yap's own llamafile engine, in
//!   `--embedding` mode, on this PC only; downloaded on demand.
//! - [`store`] + [`index`]: each passage's vector in
//!   `<data>/search-index/vectors.bin`, kept up to date in the background,
//!   never in a dictation's way.
//! - [`dates`] + [`rank`]: the question's dates, and hybrid ranking (BM25
//!   keywords + meaning, reciprocal rank fusion, the voices it names).
//! - [`ask`]: the passages for a question, as the model reads them and as
//!   sources that open the meeting at the line; the answer without a model.
//!
//! Chat's "meetings" scope (the Meetings view's Ask bar, and a conversation
//! started there) answers through [`ask`] (`chat/turn.rs`); the Meetings
//! view's search lists the moments ([`commands::search_moments`]). Settings
//! → Privacy & data → "Search my meetings by meaning" downloads the model,
//! shows the index, rebuilds and deletes it. Architecture:
//! `docs/architecture/search.md`.

pub mod ask;
pub mod chunks;
pub mod commands;
pub mod dates;
pub mod embedder;
pub mod index;
pub mod rank;
pub mod store;

use tauri::AppHandle;

/// Start the background index (it waits until it's wanted).
pub fn init(app: &AppHandle) {
    index::init(app);
}

/// A config just saved: search by meaning switched on starts a pass; off
/// stops the model's server.
pub fn on_config_saved(prev: &crate::config::YapConfig, cfg: &crate::config::YapConfig) {
    if prev.search_by_meaning == cfg.search_by_meaning {
        return;
    }
    if cfg.search_by_meaning {
        index::poke();
    } else {
        embedder::stop();
    }
}

/// Yap is quitting: the model's server stops.
pub fn shutdown() {
    embedder::shutdown();
}

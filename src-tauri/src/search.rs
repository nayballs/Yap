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
pub mod library;
pub mod rank;
pub mod store;

use tauri::AppHandle;

/// Start the background index (it waits until it's wanted).
pub fn init(app: &AppHandle) {
    index::init(app);
}

/// Something searched changed (a chat saved, a note edited outside Yap's
/// events): the index reads it in the background, a few seconds on.
#[cfg_attr(test, allow(dead_code))] // unit tests' chats are never saved
pub fn changed() {
    index::poke();
}

/// **Something was deleted** — a note, a chat, a memory item, or lines of a
/// transcript (the meeting's retention) — and the index must let go of it:
/// the vectors of every passage that's gone are dropped a few seconds on,
/// with search by meaning on or off. Call it after the store has let go.
pub fn removed() {
    index::removed();
}

/// A config just saved: search by meaning switched on starts a pass (and
/// the model's server may try to start again); off stops it.
pub fn on_config_saved(prev: &crate::config::YapConfig, cfg: &crate::config::YapConfig) {
    if prev.search_by_meaning == cfg.search_by_meaning {
        return;
    }
    if cfg.search_by_meaning {
        embedder::reset_failures();
        index::poke();
    } else {
        embedder::stop();
    }
}

/// Yap is quitting: the model's server stops.
pub fn shutdown() {
    embedder::shutdown();
}

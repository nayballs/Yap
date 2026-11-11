//! **A step's pictures** (11 Nov 2026, after Claude Code's own transcript,
//! where a step that read or made a picture shows it at the end of its
//! line): when a tool call's answer carries a picture — a picture file read,
//! a screenshot a tool took, a command that printed one — Yap keeps a copy
//! with the agent, and that step shows it in the thread (`AgentWork`).
//!
//! - **Which**: the pictures in the tool result itself, as Claude Code hands
//!   them to the model (`events::Block::ToolResult`'s `pictures`, base64): what
//!   the model saw, never a file Yap went and read.
//! - **Kept** ([`keep`]) in `agents/<id>/pictures/<img id>.png`: a picture by
//!   its own bytes (`local::kind`), decoded here (`snip::attach::decode`), at
//!   most [`LONG_EDGE`] on its long side, with the ids and files a chat
//!   screenshot has (`snip::images::save_in`) so its thumbnails are made the
//!   same way (`snip/thumbs.rs`, in `pictures/thumbs/`). In the agent's own
//!   folder, never `chat-images/`: an older Yap sharing the data folder
//!   sweeps what no conversation names there after a day.
//! - **Gone with the agent's log**: the folder's sweep 7 days after it ended
//!   (`store::sweep`), Delete some of your data → Agents (`store::reset`).
//! - **How many**: [`PER_TURN`] a turn (the runner counts), [`PER_AGENT`] an
//!   agent; past them a step keeps its line and no picture. Never in a
//!   model evaluation (as the log).
//! - **Shown** through the `yapsnip` scheme by ids only, `agent/<agent
//!   id>/<img id>` ([`address`], [`parse`]: `snip::serve`; Copy image and
//!   Save image as… take the same address).
//!
//! **Privacy**: the log has sizes and counts, never a picture's data.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use super::store;
use crate::ai::claude_code::events::Picture;
use crate::snip::{attach, images, thumbs};

/// The folder under an agent's.
pub const FOLDER: &str = "pictures";
/// Pictures one turn keeps, at most.
pub const PER_TURN: usize = 24;
/// Pictures one agent keeps, at most.
pub const PER_AGENT: usize = 120;
/// A kept picture's long side, at most (a bigger one is made smaller).
pub const LONG_EDGE: u32 = 1600;
/// A picture's most bytes, as it came.
pub const MAX_BYTES: usize = 12 * 1024 * 1024;
/// How an address starts.
const PREFIX: &str = "agent/";

/// **A picture a step's answer carried**, as the agent's log and the page
/// keep it: its id in the agent's `pictures/` and its size.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepPicture {
    pub id: String,
    pub w: u32,
    pub h: u32,
}

/// `agents/<id>/pictures/`.
pub fn dir(agent: &str) -> Option<PathBuf> {
    store::dir(agent).map(|d| d.join(FOLDER))
}

/// How many pictures `dir` keeps (its thumbnails aside).
pub fn kept_in(dir: &Path) -> usize {
    std::fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".png")).count()
}

/// **The PNG kept of a picture's bytes**, and its size: a picture by its own
/// bytes, within [`MAX_BYTES`], its long side at most [`LONG_EDGE`].
pub fn fitted(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    if bytes.len() > MAX_BYTES {
        return Err("too big".into());
    }
    if crate::chat::pictures::local::kind(bytes).is_none() {
        return Err("not a picture".into());
    }
    let (rgba, w, h) = attach::decode(bytes)?;
    let (fw, fh) = images::fit(w, h, LONG_EDGE);
    let rgba = if (fw, fh) == (w, h) { rgba } else { images::shrink(&rgba, w, h, fw, fh) };
    Ok((images::encode_png(&rgba, fw, fh, true)?, fw, fh))
}

/// Keep `picture` in `dir` (an agent's `pictures/`), unless it has
/// [`PER_AGENT`] already.
pub fn keep_in(dir: &Path, picture: &Picture) -> Result<StepPicture, String> {
    if kept_in(dir) >= PER_AGENT {
        return Err("this agent keeps no more pictures".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(picture.data.trim()).map_err(|_| "not base64".to_string())?;
    let (png, w, h) = fitted(&bytes)?;
    let saved = images::save_in(dir, &png, w, h)?;
    tracing::debug!(w, h, bytes = saved.bytes, from = bytes.len(), "Agents: a step's picture kept");
    Ok(StepPicture { id: saved.id, w, h })
}

/// **Keep a picture agent `agent`'s step was handed** (never in a model
/// evaluation).
pub fn keep(agent: &str, picture: &Picture) -> Result<StepPicture, String> {
    if crate::ai::eval::active() {
        return Err("not kept in an evaluation".into());
    }
    keep_in(&dir(agent).ok_or("not an agent's id")?, picture)
}

/// A kept picture's address, as the page writes it (core/agents.js
/// `stepPicture`): `agent/<agent id>/<img id>`. The tests'.
#[cfg(test)]
pub fn address(agent: &str, id: &str) -> String {
    format!("{PREFIX}{agent}/{id}")
}

/// **An address's agent and picture**: both Yap's own ids and nothing else
/// (no path can hide in one). Pure.
pub fn parse(address: &str) -> Option<(&str, &str)> {
    let (agent, id) = address.strip_prefix(PREFIX)?.split_once('/')?;
    (store::valid_id(agent) && images::is_id(id)).then_some((agent, id))
}

/// The PNG at `address` (`None`: not one, or gone with its agent's log).
pub fn read(address: &str) -> Option<Vec<u8>> {
    let (agent, id) = parse(address)?;
    std::fs::read(dir(agent)?.join(format!("{id}.png"))).ok()
}

/// Its thumbnail at `w` × `h`, as a chat screenshot's (`cover`: a tile
/// filled with its middle). `None`: not one of its sizes, or no such picture.
pub fn thumb(address: &str, w: u32, h: u32, cover: bool) -> Option<Vec<u8>> {
    let (agent, id) = parse(address)?;
    let dir = dir(agent)?;
    if cover { thumbs::get_cover_in(&dir, id, w, h) } else { thumbs::get_in(&dir, id, w, h) }.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_dir() -> PathBuf {
        let mut raw = [0u8; 6];
        let _ = getrandom::fill(&mut raw);
        let tag: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        std::env::temp_dir().join(format!("yap-step-pictures-{tag}"))
    }

    /// A `w` × `h` picture as Claude Code hands one over.
    fn handed(w: u32, h: u32) -> Picture {
        let rgba: Vec<u8> = (0..w * h).flat_map(|i| [(i % 251) as u8, (i % 13) as u8, 90, 255]).collect();
        let png = images::encode_png(&rgba, w, h, true).unwrap();
        Picture { media_type: "image/png".into(), data: base64::engine::general_purpose::STANDARD.encode(png) }
    }

    #[test]
    fn a_picture_is_kept_with_its_agent_no_bigger_than_its_long_edge() {
        let dir = fresh_dir();
        // One that fits: kept at its own size.
        let small = keep_in(&dir, &handed(320, 200)).unwrap();
        assert_eq!((small.w, small.h), (320, 200));
        assert!(images::is_id(&small.id));
        let png = std::fs::read(dir.join(format!("{}.png", small.id))).unwrap();
        assert_eq!(images::png_size(&png), Some((320, 200)));
        // A bigger one: its long side brought down, its shape kept.
        let big = keep_in(&dir, &handed(LONG_EDGE * 2, 500)).unwrap();
        assert_eq!((big.w, big.h), (LONG_EDGE, 250));
        assert_eq!(kept_in(&dir), 2);
        // Its thumbnail is made as a chat screenshot's, beside it (and isn't counted as a picture).
        let thumb = thumbs::get_in(&dir, &small.id, 160, 100).unwrap();
        assert_eq!(images::png_size(&thumb), Some((160, 100)));
        assert_eq!(kept_in(&dir), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_is_not_a_picture_is_not_kept() {
        let dir = fresh_dir();
        let as_base64 = |bytes: &[u8]| Picture { media_type: "image/png".into(), data: base64::engine::general_purpose::STANDARD.encode(bytes) };
        assert_eq!(keep_in(&dir, &Picture { media_type: "image/png".into(), data: "not base64 at all!".into() }), Err("not base64".into()));
        // By its bytes, whatever its type says: a page, a script, an SVG.
        assert_eq!(keep_in(&dir, &as_base64(b"<svg xmlns='http://www.w3.org/2000/svg'/>")), Err("not a picture".into()));
        assert_eq!(keep_in(&dir, &as_base64(b"MZ\x90\x00")), Err("not a picture".into()));
        // A PNG's first bytes with nothing behind them.
        assert!(keep_in(&dir, &as_base64(b"\x89PNG\r\n\x1a\nrubbish")).is_err());
        assert_eq!(fitted(&vec![0u8; MAX_BYTES + 1]), Err("too big".into()));
        assert_eq!(kept_in(&dir), 0);
        // An agent with its share already keeps no more.
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..PER_AGENT {
            std::fs::write(dir.join(format!("p{i}.png")), b"x").unwrap();
        }
        assert_eq!(keep_in(&dir, &handed(8, 8)), Err("this agent keeps no more pictures".into()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_address_is_two_ids_and_nothing_else() {
        let id = "img_0123456789abcdef";
        assert_eq!(address("19a3f0c2b7e4d1aa", id), format!("agent/19a3f0c2b7e4d1aa/{id}"));
        assert_eq!(parse(&address("19a3f0c2b7e4d1aa", id)), Some(("19a3f0c2b7e4d1aa", id)));
        for bad in [
            id,
            "agent/19a3f0c2b7e4d1aa",
            "agent//img_0123456789abcdef",
            "agent/../img_0123456789abcdef",
            "agent/19a3f0c2b7e4d1aa/../../config",
            "agent/19a3f0c2b7e4d1aa/img_0123456789abcdef/more",
            "agent/19a3f0c2b7e4d1aa/img_0123456789ABCDEF",
            "agent/ab 12/img_0123456789abcdef",
            "agents/19a3f0c2b7e4d1aa/img_0123456789abcdef",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_kept_picture_is_read_back_by_its_address_and_gone_with_its_agents_folder() {
        let agent = store::new_id();
        let kept = keep(&agent, &handed(64, 40)).unwrap();
        let at = address(&agent, &kept.id);
        assert_eq!(images::png_size(&read(&at).unwrap()), Some((64, 40)));
        // A tile filled with its middle, and a thumbnail of its own shape.
        assert_eq!(images::png_size(&thumb(&at, 30, 20, true).unwrap()), Some((30, 20)));
        assert_eq!(images::png_size(&thumb(&at, 32, 20, false).unwrap()), Some((32, 20)));
        assert_eq!(thumb(&at, 500, 500, false), None, "bigger than the picture: not a thumbnail of it");
        // Another agent's address finds nothing.
        assert_eq!(read(&address(&store::new_id(), &kept.id)), None);
        // The agent's folder gone (the 7-day sweep, Delete some of your data): its pictures too.
        std::fs::remove_dir_all(store::dir(&agent).unwrap()).unwrap();
        assert_eq!(read(&at), None);
        assert_eq!(thumb(&at, 30, 20, true), None);
    }
}

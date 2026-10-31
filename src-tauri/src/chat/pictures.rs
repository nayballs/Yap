//! Pictures Chat found online and showed beside the chat (find and show,
//! 31 Oct 2026): what an answer keeps of each (`ChatMessage::pictures`) and
//! what the page's panel shows (DocPanel's picture tab). The file itself is
//! a chat screenshot (`snip::images`), shown by its id.
//!
//! (The frontend's stand-in: the shape the contract fixes, so a seeded
//! conversation's pictures round-trip. The Rust side's own module takes its
//! place.)

use serde::{Deserialize, Serialize};

/// The page a picture was on ("From Fort Joy - Example Wiki · Open page").
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRef {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub site: String,
    #[serde(default)]
    pub url: String,
}

/// A picture an answer showed: its chat-images id and size, the model's
/// caption, the page's own alt text, where it came from and that page's
/// citation number in the answer's Sources (0: not a source).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShownPicture {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub w: u32,
    #[serde(default)]
    pub h: u32,
    #[serde(default)]
    pub caption: String,
    #[serde(default)]
    pub alt: String,
    #[serde(default)]
    pub image_url: String,
    #[serde(default)]
    pub page: PageRef,
    #[serde(default)]
    pub n: usize,
}

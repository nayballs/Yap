//! A dictation into a chat app or an AI chat box, before it's pasted:
//! stacked messages ("new message" said, `pasting/stacked.rs`), and where
//! mentions and file tags go (`pasting/mentions.rs`, `pasting/file_tags.rs`)
//! — the pieces `pasting/compose.rs` pastes, types and picks in steps.

use std::sync::Arc;

use super::Shared;
use crate::dictionary::screen_terms::ScreenTerms;
use crate::pasting::stacked::{self, Stacked};

/// What `deliver_text` needs for a dictation in steps: whether it goes
/// into a chat app (by its process or its tab, as cleanup's styles tell),
/// and the names on screen it read.
pub(super) struct Steps {
    pub chat: bool,
    pub screen: Option<Arc<ScreenTerms>>,
}

impl Shared {
    /// **Stacked messages**, when they're on (`config.stacked_messages`):
    /// "new message" said in a chat app's dictation. Sent anyway (`sends`):
    /// the messages, each to be pasted and sent. Not sent: `corrected` with
    /// the messages on lines of their own (and no words as said to swap it
    /// for). `None`: one message, as ever.
    pub(super) fn stacked(&self, corrected: &mut String, said: &mut Option<String>, chat: bool, sends: bool) -> Option<Vec<String>> {
        let on = self.config.read().map(|c| c.stacked_messages).unwrap_or(false);
        if !on || corrected.trim().is_empty() {
            return None;
        }
        match stacked::decide(corrected, said.as_deref(), chat, sends) {
            Stacked::AsIs => None,
            Stacked::Lines(text) => {
                tracing::info!(lines = text.lines().count(), "Stacked messages: not sent, so on lines of their own");
                *corrected = text;
                *said = None;
                None
            }
            Stacked::Send(messages) => {
                tracing::info!(messages = messages.len(), "Stacked messages: each pasted and sent");
                *said = None;
                Some(messages)
            }
        }
    }

    /// Where this dictation's mentions and file tags go, as the settings
    /// have it: a team chat's mentions of the people on screen ("Turn “at
    /// Sam” into a mention"), an AI chat box's or a coding CLI's files ("Tag
    /// files in AI chat boxes"). Nothing without the screen's names.
    pub(super) fn compose_place(&self, screen: Option<&ScreenTerms>) -> crate::pasting::compose::Where {
        let mut place = crate::pasting::compose::Where::default();
        let Some(screen) = screen else { return place };
        let (mentions_on, tags_on) = self.config.read().map(|c| (c.chat_mentions, c.file_tags)).unwrap_or((false, false));
        if tags_on {
            if let Some(code) = &screen.code {
                place.tagging = crate::pasting::file_tags::tagging(code);
                if place.tagging.is_some() {
                    place.files = code.files.clone();
                }
            }
        }
        if mentions_on && place.tagging.is_none() {
            let target = match self.target_hwnd.load(std::sync::atomic::Ordering::Relaxed) {
                0 => None,
                h => Some(h),
            };
            let (exe, title) = crate::pasting::compose::app_and_title(target);
            place.mentions = crate::pasting::mentions::which(&exe, &title);
            if let Some(app) = place.mentions {
                place.people = crate::pasting::mentions::people(&screen.terms);
                tracing::debug!(app = app.name(), people = place.people.len(), "Mentions: the people on screen");
            }
        }
        place
    }
}

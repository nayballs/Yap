//! The loop's budget: what a turn's request may still take
//! (`chat/budget.rs`), each tool's result fitted to it and each picture a
//! step looks at counted in it.

use serde_json::Value;

/// What a turn's request may still take (`chat/budget.rs`): a tool's result
/// is trimmed to it, so one turn's page reads and notes can't overflow the
/// model's context — beside the pictures the request carries, each by its
/// size where it goes (`budget::images_in`).
#[derive(Clone, Copy)]
pub(super) struct Room<'a> {
    /// The whole request's limit (`Budget::request`); `None`: no limit.
    pub(super) limit: Option<u64>,
    /// The tools the steps offer (they're sent every step).
    pub(super) tools: &'a Value,
    /// Where the steps go: what a picture costs there.
    pub(super) provider: &'a str,
    /// The model runs on this PC.
    pub(super) here: bool,
}

/// What a picture a step looked at must leave after it: its tool result and
/// the next step's call.
pub(super) const LOOK_SPARE: u64 = 300;

impl Room<'_> {
    /// What the request has left beside `messages` and the pictures this
    /// step looked at, still to go after its results (`pending`).
    fn left(&self, messages: &[Value], pending: &[crate::chat::pictures::Look]) -> Option<u64> {
        let used = crate::chat::budget::request_tokens(messages, self.tools, self.provider, self.here) + pending.iter().map(|l| self.look_tokens(l)).sum::<u64>();
        self.limit.map(|limit| limit.saturating_sub(used))
    }

    /// What a picture looked at adds: the picture, Yap's line with it and
    /// its message.
    pub(super) fn look_tokens(&self, look: &crate::chat::pictures::Look) -> u64 {
        crate::chat::budget::part_tokens(&look.part, self.provider, self.here) + crate::chat::budget::estimate(&look.text) + crate::chat::budget::MESSAGE_OVERHEAD
    }

    /// `result` as the next tool message carries it: within what's left
    /// beside `messages` and the step's `pending` pictures
    /// (`budget::fit_result`).
    pub(super) fn fit(&self, result: &Value, messages: &[Value], pending: &[crate::chat::pictures::Look]) -> String {
        let Some(left) = self.left(messages, pending) else { return result.to_string() };
        let left = left.saturating_sub(crate::chat::budget::MESSAGE_OVERHEAD);
        let fitted = crate::chat::budget::fit_result(result, left);
        if fitted.len() < result.to_string().len() {
            tracing::info!(left, "Chat: a tool's result was shortened to fit the model's context");
            crate::ai::eval::post_fix("tool_result_cut");
        }
        fitted
    }

    /// Whether the picture `look` still fits beside `messages` and the
    /// step's `pending` pictures, with [`LOOK_SPARE`] after it. A picture
    /// can't be cut as a text is: one that doesn't fit isn't looked at.
    pub(super) fn takes_picture(&self, messages: &[Value], pending: &[crate::chat::pictures::Look], look: &crate::chat::pictures::Look) -> bool {
        self.left(messages, pending).is_none_or(|left| self.look_tokens(look) + LOOK_SPARE <= left)
    }
}

//! **One call to a connector's tool**: sent with a progress token, waited
//! for at most [`DEFAULT_TIMEOUT`] (a connector can set up to [`MAX_TIMEOUT`];
//! a progress notification from the server starts the clock again, never
//! past [`MAX_TOTAL`]), stopped when its [`Stop`] fires (■ Stop, a newer
//! question) or its future is dropped, and its answer shaped (`results.rs`).
//!
//! However it ends early — a timeout, a stop, the turn dropped — the server
//! is told with `notifications/cancelled` for that request (over stdio and
//! HTTP alike; on HTTP the request's stream closes with it). rmcp sends it
//! for its own timeout; [`Abandon`] for the rest.
//!
//! Two turns at once (Chat and the Voice Assistant) share one connection:
//! each call is its own request id. A call never retries: whether a write
//! happened when the line broke can't be known ([`CallError::Lost`]), and
//! Chat's tool loop says so ("It may or may not have done it"); it never
//! calls again by itself.
//!
//! Chat's tool loop calls through here (`chat/tools/connectors.rs`, after
//! its consent, `connectors::call_for_chat`), and a debug build's
//! `connector_call_tool`.
#![cfg_attr(not(debug_assertions), allow(dead_code))]

use std::time::Duration;

use rmcp::model::{CallToolRequest, CallToolRequestParams, CancelledNotificationParam, ClientRequest, JsonObject, RequestId, ServerResult};
use rmcp::service::{Peer, PeerRequestOptions, RoleClient, ServiceError};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::results::{self, Shaped};

/// How long a call waits by default.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
/// The most a connector's own timeout may be.
pub const MAX_TIMEOUT: Duration = Duration::from_secs(600);
/// However much progress a server reports, a call ends by then.
pub const MAX_TOTAL: Duration = Duration::from_secs(30 * 60);

/// A call's stop: fired by ■ Stop or a newer question (`cancel`), or by
/// dropping the call's future.
pub type Stop = CancellationToken;

/// How a call didn't give an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// Stopped: by the person, or a newer question.
    Stopped,
    /// No answer in time (or no progress for that long).
    TimedOut(Duration),
    /// The connection went before the answer came: it may or may not have
    /// happened.
    Lost,
    /// The server asked for more input than Yap can give yet (MCP's
    /// `input_required`); forms come later.
    NeedsInput,
    /// The server refused the call itself (an unknown tool, bad arguments):
    /// its message, as data.
    Refused(String),
}

impl CallError {
    /// One word for the log.
    pub fn word(&self) -> &'static str {
        match self {
            CallError::Stopped => "stopped",
            CallError::TimedOut(_) => "timed-out",
            CallError::Lost => "lost",
            CallError::NeedsInput => "needs-input",
            CallError::Refused(_) => "refused",
        }
    }

    /// In plain words, for `who` ("A connector", or a catalogue name).
    pub fn words(&self, who: &str) -> String {
        match self {
            CallError::Stopped => "Stopped.".into(),
            CallError::TimedOut(t) => format!("No answer from {who} in {} s, so Yap stopped waiting.", (t.as_millis() as u64).div_ceil(1000)),
            CallError::Lost => format!("Didn't hear back from {who}. It may or may not have done it."),
            CallError::NeedsInput => format!("{who} asked for more than Yap can give yet."),
            CallError::Refused(m) => format!("{who} refused the call: {m}"),
        }
    }
}

/// The timeout for a connector that set `secs` (none: the default), within
/// [`MAX_TIMEOUT`]. Pure.
pub fn timeout_for(secs: Option<u64>) -> Duration {
    secs.map_or(DEFAULT_TIMEOUT, |s| Duration::from_secs(s.clamp(1, MAX_TIMEOUT.as_secs())))
}

/// Tells the server a request was given up on, unless it finished
/// ([`Abandon::done`]): when the call's future is dropped mid-way (a turn
/// stopped, a `select!` lost) as much as on [`Stop`].
struct Abandon {
    peer: Peer<RoleClient>,
    id: Option<RequestId>,
}

impl Abandon {
    fn done(&mut self) {
        self.id = None;
    }
}

impl Drop for Abandon {
    fn drop(&mut self) {
        let Some(id) = self.id.take() else { return };
        let peer = self.peer.clone();
        let send = async move {
            let _ = peer.notify_cancelled(CancelledNotificationParam::new(Some(id), Some("Stopped in Yap".into()))).await;
        };
        // Only ever dropped on the runtime the call ran on.
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            rt.spawn(send);
        }
    }
}

/// Call `tool` with `args` (an object; anything else is sent as none) on
/// `peer`, waiting at most `timeout` between signs of life, until `stop`.
/// The answer shaped, or why there isn't one.
pub async fn call(peer: &Peer<RoleClient>, tool: &str, args: &Value, timeout: Duration, stop: &Stop) -> Result<Shaped, CallError> {
    let mut params = CallToolRequestParams::new(tool.to_string());
    if let Value::Object(map) = args {
        params = params.with_arguments(map.clone() as JsonObject);
    }
    let options = {
        let mut o = PeerRequestOptions::with_timeout(timeout).reset_timeout_on_progress();
        o.max_total_timeout = Some(MAX_TOTAL.max(timeout));
        o
    };
    let request = ClientRequest::CallToolRequest(CallToolRequest::new(params));
    let handle = tokio::select! {
        biased;
        _ = stop.cancelled() => return Err(CallError::Stopped),
        h = peer.send_request_with_option(request, options) => h.map_err(|e| error_of(&e))?,
    };
    let mut abandon = Abandon { peer: peer.clone(), id: Some(handle.id.clone()) };
    let answer = tokio::select! {
        biased;
        _ = stop.cancelled() => return Err(CallError::Stopped),
        r = handle.await_response() => r,
    };
    // Answered, or rmcp's own timeout (which told the server itself).
    abandon.done();
    match answer {
        Ok(ServerResult::CallToolResult(result)) => {
            let json = serde_json::to_value(&result).unwrap_or(Value::Null);
            Ok(results::shape(&json))
        }
        Ok(ServerResult::InputRequiredResult(_)) => Err(CallError::NeedsInput),
        Ok(_) => Err(CallError::Refused("its answer wasn't a tool's result".into())),
        Err(e) => Err(error_of(&e)),
    }
}

/// rmcp's error as a [`CallError`].
fn error_of(e: &ServiceError) -> CallError {
    match e {
        ServiceError::Timeout { timeout } => CallError::TimedOut(*timeout),
        ServiceError::McpError(data) => CallError::Refused(data.message.chars().take(500).collect()),
        ServiceError::Cancelled { .. } => CallError::Stopped,
        _ => CallError::Lost,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connectors_timeout_stays_within_its_bounds() {
        assert_eq!(timeout_for(None), DEFAULT_TIMEOUT);
        assert_eq!(timeout_for(Some(5)), Duration::from_secs(5));
        assert_eq!(timeout_for(Some(0)), Duration::from_secs(1));
        assert_eq!(timeout_for(Some(100_000)), MAX_TIMEOUT);
        assert!(CallError::Lost.words("A connector").contains("may or may not"));
        assert_eq!(CallError::TimedOut(Duration::from_secs(60)).words("A connector"), "No answer from A connector in 60 s, so Yap stopped waiting.");
    }
}

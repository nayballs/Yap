//! **What a Chat run reads**, in process: the meetings, notes, tasks and
//! folders as `yap.exe mcp` reads them (the Local API's read routes,
//! `bridge::read_path`, without the HTTP, so the Local API may be off),
//! Yap's memory (`search::library::memory_block`: its keyword rule, and
//! meaning when search by meaning is on) and the calendar
//! (`chat::tools::calendar`, the lookup Chat's `check_calendar` runs, in
//! the person's own time zone). Read only, never the dictation history.
//!
//! A test hands the endpoint its own [`Reads`].

use std::sync::Arc;

use serde_json::{json, Value};

use crate::integrations::mcp::{Bridge, BridgeError};

/// Yap's memory, searched.
pub const MEMORY_TOOL: &str = "search_memory";
/// The calendar, as Chat's tool reads it.
pub const CALENDAR_TOOL: &str = crate::chat::tools::calendar::NAME;

/// **The reads a Chat run's tools make.** Blocking; each runs on its
/// request's own thread.
pub trait Reads: Send + Sync + 'static {
    /// A Local API read route's JSON body (`GET /v1/meetings/list?…`), for
    /// `mcp::run_tool`.
    fn bridge(&self, method: &str, path: &str) -> Result<Value, BridgeError>;
    /// The memory items about `query`, as a block of text; `None`: nothing
    /// matches.
    fn memory(&self, query: &str) -> Option<String>;
    /// A calendar is connected (the run is offered `check_calendar`).
    fn calendar_connected(&self) -> bool;
    /// `check_calendar`'s answer for the model, or what went wrong.
    fn calendar(&self, args: &Value) -> Result<String, String>;
}

/// `mcp::run_tool`'s bridge over some [`Reads`].
pub struct Via(pub Arc<dyn Reads>);

impl Bridge for Via {
    fn call(&self, method: &str, path: &str, _body: Option<&Value>) -> Result<Value, BridgeError> {
        self.0.bridge(method, path)
    }
}

/// **The app's own reads.**
pub struct InApp;

impl Reads for InApp {
    fn bridge(&self, method: &str, path: &str) -> Result<Value, BridgeError> {
        if method != "GET" {
            return Err(BridgeError::Api { status: 403, message: "Yap's tools here only read.".into() });
        }
        // Saving notes is never offered to a run: `create_note` asks this.
        if path.starts_with("/v1/mcp/config") {
            return Ok(json!({ "data": { "allowWrites": false } }));
        }
        match crate::integrations::bridge::read_path(path) {
            Some(Ok(v)) => Ok(v),
            Some(Err((status, message))) => Err(BridgeError::Api { status, message }),
            None => Err(BridgeError::Api { status: 404, message: "Not found".into() }),
        }
    }

    fn memory(&self, query: &str) -> Option<String> {
        // Off the runtime's own threads (this is a request's thread), as
        // Chat's turn awaits it.
        tauri::async_runtime::block_on(crate::search::library::memory_block(query))
    }

    fn calendar_connected(&self) -> bool {
        crate::calendar::agenda().connected
    }

    fn calendar(&self, args: &Value) -> Result<String, String> {
        use crate::chat::tools::calendar;
        let cfg = crate::config::load();
        let zone = calendar::Zone::of(crate::ai::about_you::profile(&cfg).region.time_zone.as_deref());
        let now = chrono::Utc::now().timestamp();
        let mut n = 0;
        let mut next = || {
            n += 1;
            n
        };
        let result = calendar::run(args, &calendar::Log::default(), zone, now, &mut next);
        render_calendar(&result)
    }
}

/// `check_calendar`'s result (`{success, data, error}`) for the model: the
/// lookup in plain lines (`calendar::plain`), or its error. Pure.
pub fn render_calendar(result: &Value) -> Result<String, String> {
    if result["success"] == json!(true) {
        Ok(crate::chat::tools::calendar::plain(&result["data"]))
    } else {
        Err(result["error"].as_str().unwrap_or("Yap couldn't read the calendar.").to_string())
    }
}

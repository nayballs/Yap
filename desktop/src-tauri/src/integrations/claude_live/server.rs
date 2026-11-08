//! The listener: `127.0.0.1` on a port the system picks, one acceptor
//! thread that checks every request before anything else (from this PC,
//! this PC's `Host`, no other site's `Origin`, a registered run, its key),
//! then a thread per accepted request — so a permission prompt waiting for
//! the person never holds up a read or a backup. Nothing unchecked gets a
//! thread or has its body read.

use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};
use tiny_http::{Header, Method, Request, Response, Server};

use super::approve::Book;
use super::reads::Reads;
use super::{RunHandle, RunSpec};

/// The largest request body read: a `Write` of a big file carries its whole
/// text, in the hook's input and in a permission prompt's.
pub const MAX_BODY: usize = 64 * 1024 * 1024;

/// An app event, sent to Yap's windows (a test keeps them).
pub type Emit = Arc<dyn Fn(&str, Value) + Send + Sync>;

/// A registered run.
pub struct Run {
    pub id: String,
    /// SHA-256 of `Bearer <key>`: compared hashed, so the time it takes
    /// says nothing about the key.
    digest: Vec<u8>,
    pub spec: RunSpec,
    /// One backup at a time per run (Claude Code can edit files in
    /// parallel), so the first copy of a file is the one kept.
    pub backups: Mutex<()>,
    /// The chat turn its calls belong to (`turn.rs`), shared with its handle.
    pub turn: Arc<super::turn::Slot>,
}

/// **The endpoint**: the listener, its runs, its permission prompts.
pub struct Endpoint {
    server: Arc<Server>,
    pub port: u16,
    runs: Mutex<HashMap<String, Arc<Run>>>,
    pub reads: Arc<dyn Reads>,
    pub emit: Emit,
    pub approvals: Book,
    /// How long a permission prompt waits ([`super::APPROVE_WAIT`]; a test's
    /// is short).
    pub wait: Duration,
    /// The largest file the backup hook copies ([`super::backup::MAX_BACKUP_BYTES`]).
    pub max_backup: u64,
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        self.server.unblock();
    }
}

/// 32 random bytes as hex.
fn random_hex(bytes: usize) -> Result<String, String> {
    let mut raw = vec![0u8; bytes];
    getrandom::fill(&mut raw).map_err(|e| format!("No random numbers on this PC for a Claude Code run's key: {e}"))?;
    Ok(raw.iter().map(|b| format!("{b:02x}")).collect())
}

fn digest(s: &str) -> Vec<u8> {
    Sha256::digest(s.as_bytes()).to_vec()
}

impl Endpoint {
    /// Bind `127.0.0.1:0` and start accepting.
    pub fn start(reads: Arc<dyn Reads>, emit: Emit, wait: Duration) -> Result<Arc<Endpoint>, String> {
        Self::start_with(reads, emit, wait, super::backup::MAX_BACKUP_BYTES)
    }

    /// [`Endpoint::start`] with its own backup limit (a test's is small).
    pub fn start_with(reads: Arc<dyn Reads>, emit: Emit, wait: Duration, max_backup: u64) -> Result<Arc<Endpoint>, String> {
        let server = Server::http("127.0.0.1:0").map_err(|e| format!("Yap couldn't open its endpoint for Claude Code: {e}"))?;
        let port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .ok_or("Yap's endpoint for Claude Code has no address")?;
        let server = Arc::new(server);
        let ep = Arc::new(Endpoint {
            server: Arc::clone(&server),
            port,
            runs: Mutex::new(HashMap::new()),
            reads,
            emit,
            approvals: Book::default(),
            wait,
            max_backup,
        });
        let weak = Arc::downgrade(&ep);
        std::thread::Builder::new()
            .name("yap-claude-live".into())
            .spawn(move || accept(server, weak))
            .map_err(|e| format!("Yap couldn't start its endpoint for Claude Code: {e}"))?;
        tracing::info!(port, "Claude Code endpoint: listening on 127.0.0.1");
        Ok(ep)
    }

    /// A run's id and key, kept until its handle drops.
    pub fn register(self: &Arc<Self>, spec: RunSpec) -> Result<RunHandle, String> {
        let id = random_hex(16)?;
        let key = random_hex(32)?;
        let tools = spec.tools;
        let agents = spec.agents && tools == super::Tools::Chat;
        let memory = spec.memory;
        // The screen, the pages and the connectors: a Chat run's only.
        let mut spec = spec;
        if tools != super::Tools::Chat {
            spec.eyes = super::eyes::Eyes::default();
            spec.connectors = None;
        }
        let eyes = spec.eyes;
        let connectors: Vec<String> = spec.connectors.as_ref().map(|l| l.names().map(str::to_string).collect()).unwrap_or_default();
        let has_backup = spec.backup_dir.is_some();
        let turn = Arc::new(super::turn::Slot::default());
        let run = Arc::new(Run { id: id.clone(), digest: digest(&format!("Bearer {key}")), spec, backups: Mutex::new(()), turn: Arc::clone(&turn) });
        let count = {
            let mut runs = self.runs.lock().unwrap_or_else(|p| p.into_inner());
            runs.insert(id.clone(), run);
            runs.len()
        };
        tracing::info!(run = %id, tools = ?tools, agents, eyes = eyes.any(), connectors = connectors.len(), backup = has_backup, runs = count, "Claude Code endpoint: a run registered");
        Ok(RunHandle { endpoint: Arc::clone(self), id, key, tools, agents, memory, eyes, connectors, has_backup, turn })
    }

    /// The run's URLs answer 404 from now on; its prompts still waiting
    /// are denied.
    pub fn unregister(&self, id: &str) {
        let removed = self.runs.lock().unwrap_or_else(|p| p.into_inner()).remove(id).is_some();
        let denied = self.approvals.end_run(id);
        if removed {
            tracing::info!(run = %id, prompts_denied = denied, "Claude Code endpoint: a run ended");
        }
    }

    fn run(&self, id: &str) -> Option<Arc<Run>> {
        self.runs.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned()
    }

    /// The checks before a request gets a thread: what it asks for, or the
    /// answer that refuses it.
    fn admit(&self, request: &Request) -> Result<(Arc<Run>, Route), (u16, &'static str)> {
        if !request.remote_addr().is_some_and(|a| a.ip().is_loopback()) {
            return Err((403, "Forbidden"));
        }
        let header = |name: &'static str| {
            request
                .headers()
                .iter()
                .find(|h| h.field.equiv(name))
                .map(|h| h.value.as_str().to_string())
        };
        if !header("Host").is_some_and(|h| host_is_here(&h, self.port)) {
            return Err((403, "Forbidden"));
        }
        if header("Origin").is_some_and(|o| !origin_is_here(&o, self.port)) {
            return Err((403, "Forbidden"));
        }
        let path = request.url().split('?').next().unwrap_or("");
        let segs: Vec<&str> = path.trim_matches('/').split('/').collect();
        let (run, route) = match segs.as_slice() {
            ["mcp", run] => (*run, Route::Mcp),
            ["hook", run, event] => (*run, Route::Hook(event.to_string())),
            _ => return Err((404, "Not found")),
        };
        let run = self.run(run).ok_or((404, "Not found"))?;
        let auth = header("Authorization").unwrap_or_default();
        if digest(&auth) != run.digest {
            return Err((401, "Unauthorized"));
        }
        Ok((run, route))
    }

    /// One accepted request, on its own thread.
    fn handle(&self, run: &Run, route: Route, mut request: Request) {
        if *request.method() != Method::Post {
            // No SSE stream and no sessions to end: the transport allows 405.
            let _ = request.respond(text(405, "Method Not Allowed").with_header(header("Allow", "POST")));
            return;
        }
        let mut body = Vec::new();
        if request.as_reader().take(MAX_BODY as u64 + 1).read_to_end(&mut body).is_err() {
            let _ = request.respond(text(400, "Unreadable request body"));
            return;
        }
        if body.len() > MAX_BODY {
            let _ = request.respond(text(413, "Request body too large"));
            return;
        }
        match route {
            Route::Mcp => match super::rpc::handle(self, run, &body) {
                super::rpc::Reply::Json(status, v) => {
                    let _ = request.respond(json(status, &v));
                }
                super::rpc::Reply::Accepted => {
                    let _ = request.respond(Response::empty(202));
                }
            },
            Route::Hook(event) => {
                // Never blocks or denies the tool: `{}` whatever happened.
                if event == "PreToolUse" {
                    if let Ok(input) = serde_json::from_slice::<Value>(&body) {
                        super::backup::pre_tool_use(run, &input, self.max_backup);
                    }
                }
                let _ = request.respond(json(200, &serde_json::json!({})));
            }
        }
    }
}

/// What an admitted request asks for.
enum Route {
    Mcp,
    Hook(String),
}

fn accept(server: Arc<Server>, endpoint: Weak<Endpoint>) {
    for request in server.incoming_requests() {
        let Some(ep) = endpoint.upgrade() else {
            let _ = request.respond(text(503, "Yap is closing"));
            break;
        };
        match ep.admit(&request) {
            Err((status, why)) => {
                tracing::debug!(status, "Claude Code endpoint: a request refused");
                let _ = request.respond(text(status, why));
            }
            Ok((run, route)) => {
                let worker = Arc::clone(&ep);
                // The request moves into the thread; if the thread can't
                // start, the request is dropped and its connection closed.
                let spawned = std::thread::Builder::new()
                    .name("yap-claude-live-req".into())
                    .spawn(move || worker.handle(&run, route, request));
                if spawned.is_err() {
                    tracing::warn!("Claude Code endpoint: no thread for a request");
                }
            }
        }
    }
}

/// `127.0.0.1`, `localhost` or `[::1]`, with this port or none: never a
/// name a web page could have pointed here.
fn host_is_here(host: &str, port: u16) -> bool {
    let host = host.trim().to_ascii_lowercase();
    let (name, at) = match host.rsplit_once(':') {
        Some((name, p)) if !name.is_empty() && !p.contains(']') => (name.to_string(), Some(p.to_string())),
        _ => (host.clone(), None),
    };
    let name_ok = matches!(name.as_str(), "127.0.0.1" | "localhost" | "[::1]");
    name_ok && at.is_none_or(|p| p == port.to_string())
}

/// An `Origin` is allowed only when it's this endpoint itself.
fn origin_is_here(origin: &str, port: u16) -> bool {
    let o = origin.trim().to_ascii_lowercase();
    o.strip_prefix("http://").is_some_and(|rest| host_is_here(rest, port) && rest.contains(':'))
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("a valid header")
}

fn text(status: u16, body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body)
        .with_status_code(status)
        .with_header(header("Content-Type", "text/plain; charset=utf-8"))
}

fn json(status: u16, body: &Value) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body.to_string())
        .with_status_code(status)
        .with_header(header("Content-Type", "application/json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_this_pc_by_name_or_address_is_here() {
        assert!(host_is_here("127.0.0.1:4100", 4100));
        assert!(host_is_here("localhost:4100", 4100));
        assert!(host_is_here("LOCALHOST", 4100));
        assert!(host_is_here("[::1]:4100", 4100));
        assert!(host_is_here("[::1]", 4100));
        assert!(!host_is_here("127.0.0.1:4101", 4100), "another port");
        assert!(!host_is_here("evil.example.com:4100", 4100), "a name that resolves here");
        assert!(!host_is_here("127.0.0.1.example.com", 4100));
        assert!(!host_is_here("", 4100));
        assert!(origin_is_here("http://127.0.0.1:4100", 4100));
        assert!(!origin_is_here("https://app.example.com", 4100));
        assert!(!origin_is_here("null", 4100));
        assert!(!origin_is_here("http://127.0.0.1", 4100), "an origin always has its port here");
    }
}

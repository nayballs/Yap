//! Local API bridge — the CLI/agent loopback server (OpenWhispr `cliBridge.js`
//! port, Rust edition).
//!
//! A tiny token-authenticated HTTP server bound to `127.0.0.1` on an
//! OS-assigned port, exposing Yap's notes, folders, and transcription history
//! as a stable `/v1` REST surface so terminals, scripts, and coding agents can
//! operate on Yap data while the app runs. Discovery matches OpenWhispr: on
//! start the app writes `{version, port, token}` to `~/.yap/cli-bridge.json`
//! (0600 on Unix) and removes it on stop — external tools read that file, then
//! send `Authorization: Bearer <token>`.
//!
//! Differences from the OpenWhispr original, all deliberate:
//! - OS-assigned port (`127.0.0.1:0`) instead of scanning 8200-8219 — the
//!   bridge file is the discovery mechanism, so a fixed range only adds
//!   collision risk (e.g. with an OpenWhispr install on the same machine).
//! - No `/audio` delete route (Yap doesn't retain dictation audio).
//! - Transcription ids are the history entry's unix-seconds `ts` (Yap's
//!   history has no row ids).
//! - Note mutations emit `yap-notes-changed` so the NotesView refreshes live
//!   (OpenWhispr broadcasts `note-added`/`note-updated` over IPC).
//!
//! Toggled by `config.bridge_enabled` (Integrations view); `sync()` is called
//! at setup and on every config save.

use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};
use tiny_http::{Method, Response, Server};

const BRIDGE_FILE_VERSION: u32 = 1;
const MAX_BODY_BYTES: usize = 1024 * 1024;

struct Running {
    server: Arc<Server>,
    port: u16,
    thread: Option<std::thread::JoinHandle<()>>,
}

static STATE: Mutex<Option<Running>> = Mutex::new(None);

/// `~/.yap/cli-bridge.json` — a fixed, well-known path (NOT the data dir, and
/// portable mode doesn't move it) so external tools can find the bridge
/// without knowing how Yap was installed. Same contract as OpenWhispr's
/// `~/.openwhispr/cli-bridge.json`.
pub fn bridge_file_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(crate::config::data_dir)
        .join(".yap")
        .join("cli-bridge.json")
}

/// Start/stop the bridge to match the config toggle. Idempotent.
pub fn sync(app: &AppHandle, enabled: bool) {
    if enabled {
        start(app);
    } else {
        stop();
    }
}

/// Status for the Integrations view: `{ running, port, bridgeFile }`.
pub fn status() -> Value {
    let guard = lock_state();
    json!({
        "running": guard.is_some(),
        "port": guard.as_ref().map(|r| r.port),
        "bridgeFile": bridge_file_path().to_string_lossy(),
    })
}

fn lock_state() -> std::sync::MutexGuard<'static, Option<Running>> {
    match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub fn start(app: &AppHandle) {
    let mut guard = lock_state();
    if guard.is_some() {
        return;
    }

    let mut raw = [0u8; 32];
    if let Err(e) = getrandom::fill(&mut raw) {
        tracing::error!("bridge: token generation failed: {}", e);
        return;
    }
    let token: String = raw.iter().map(|b| format!("{:02x}", b)).collect();

    let server = match Server::http("127.0.0.1:0") {
        Ok(s) => Arc::new(s),
        Err(e) => {
            tracing::error!("bridge: failed to bind loopback server: {}", e);
            return;
        }
    };
    let port = match server.server_addr().to_ip() {
        Some(addr) => addr.port(),
        None => {
            tracing::error!("bridge: no IP listen address");
            return;
        }
    };

    if let Err(e) = write_bridge_file(port, &token) {
        tracing::error!("bridge: failed to write bridge file: {}", e);
        return;
    }

    let srv = server.clone();
    let app = app.clone();
    let thread = std::thread::Builder::new()
        .name("yap-bridge".into())
        .spawn(move || {
            for request in srv.incoming_requests() {
                handle_request(request, &token, &app);
            }
        })
        .ok();

    *guard = Some(Running {
        server,
        port,
        thread,
    });
    tracing::info!("bridge: local API listening on 127.0.0.1:{}", port);
}

pub fn stop() {
    let running = lock_state().take();
    if let Some(mut running) = running {
        running.server.unblock();
        if let Some(t) = running.thread.take() {
            let _ = t.join();
        }
        remove_bridge_file();
        tracing::info!("bridge: stopped");
    }
}

fn write_bridge_file(port: u16, token: &str) -> std::io::Result<()> {
    let path = bridge_file_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let payload = serde_json::to_string(&json!({
        "version": BRIDGE_FILE_VERSION,
        "port": port,
        "token": token,
    }))
    .expect("bridge file payload serializes");
    std::fs::write(&path, payload)?;
    // Owner-only on Unix; Windows home-dir ACLs already restrict to the user.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn remove_bridge_file() {
    if let Err(e) = std::fs::remove_file(bridge_file_path()) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::debug!("bridge: file removal failed: {}", e);
        }
    }
}

// ---- request handling ----

fn respond_json(request: tiny_http::Request, status: u16, body: &Value) {
    let response = Response::from_string(body.to_string())
        .with_status_code(status)
        .with_header(
            tiny_http::Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .expect("static header"),
        );
    let _ = request.respond(response);
}

fn respond_error(request: tiny_http::Request, status: u16, code: &str, message: &str) {
    respond_json(
        request,
        status,
        &json!({ "error": { "code": code, "message": message } }),
    );
}

fn respond_no_content(request: tiny_http::Request) {
    let _ = request.respond(Response::empty(204));
}

/// Compare secrets without leaking length/prefix timing: hash both sides.
fn token_matches(supplied: &str, expected: &str) -> bool {
    Sha256::digest(supplied.as_bytes()) == Sha256::digest(expected.as_bytes())
}

/// Percent-decode a query value ('+' = space).
fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => {
                if let (Some(h), Some(l)) = (
                    bytes.get(i + 1).and_then(|b| (*b as char).to_digit(16)),
                    bytes.get(i + 2).and_then(|b| (*b as char).to_digit(16)),
                ) {
                    out.push((h * 16 + l) as u8);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (url_decode(k), url_decode(v)),
            None => (url_decode(p), String::new()),
        })
        .collect()
}

fn query_get<'a>(params: &'a [(String, String)], key: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn handle_request(mut request: tiny_http::Request, token: &str, app: &AppHandle) {
    // Belt-and-braces: the socket is bound to 127.0.0.1, but verify anyway
    // (mirrors the OpenWhispr loopback check).
    let is_loopback = request
        .remote_addr()
        .map(|a| a.ip().is_loopback())
        .unwrap_or(false);
    if !is_loopback {
        respond_error(request, 403, "forbidden", "Forbidden");
        return;
    }

    let auth = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .map(|h| h.value.as_str().to_string())
        .unwrap_or_default();
    let expected = format!("Bearer {}", token);
    if !token_matches(&auth, &expected) {
        respond_error(request, 401, "unauthorized", "Unauthorized");
        return;
    }

    let url = request.url().to_string();
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (url, String::new()),
    };
    let params = parse_query(&query);
    let method = request.method().clone();

    // Read + parse the JSON body for mutating verbs (capped).
    let mut body = Value::Object(Default::default());
    if !matches!(method, Method::Get | Method::Delete) {
        let mut raw = String::new();
        let mut limited = request.as_reader().take((MAX_BODY_BYTES + 1) as u64);
        if limited.read_to_string(&mut raw).is_err() {
            respond_error(request, 400, "validation_error", "Unreadable request body");
            return;
        }
        if raw.len() > MAX_BODY_BYTES {
            respond_error(request, 400, "validation_error", "Request body too large");
            return;
        }
        if !raw.trim().is_empty() {
            match serde_json::from_str(&raw) {
                Ok(v) => body = v,
                Err(_) => {
                    respond_error(request, 400, "validation_error", "Invalid JSON payload");
                    return;
                }
            }
        }
    }

    let segments: Vec<String> = path
        .trim_matches('/')
        .split('/')
        .map(|s| s.to_string())
        .collect();
    let segs: Vec<&str> = segments.iter().map(|s| s.as_str()).collect();

    route(request, &method, &segs, &params, &body, app);
}

/// Envelope helpers matching OpenWhispr's v1 API shape.
fn list_envelope(data: Value) -> Value {
    json!({ "data": data, "has_more": false, "next_cursor": null })
}

fn str_field<'a>(body: &'a Value, key: &str) -> Option<&'a str> {
    body.get(key).and_then(|v| v.as_str())
}

fn route(
    request: tiny_http::Request,
    method: &Method,
    segs: &[&str],
    params: &[(String, String)],
    body: &Value,
    app: &AppHandle,
) {
    let limit = |default: usize| {
        query_get(params, "limit")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(default)
    };

    match (method, segs) {
        (Method::Get, ["v1", "health"]) => {
            respond_json(
                request,
                200,
                &json!({ "data": { "ok": true, "version": BRIDGE_FILE_VERSION, "app": "yap" } }),
            );
        }

        // ---- notes ----
        (Method::Get, ["v1", "notes", "list"]) => {
            let folder = query_get(params, "folder");
            let note_type = query_get(params, "note_type");
            let mut items: Vec<Value> = match crate::notes::list() {
                Value::Array(a) => a,
                _ => Vec::new(),
            };
            if let Some(f) = folder {
                items.retain(|n| {
                    n.get("folder").and_then(|v| v.as_str()).map(|v| v.eq_ignore_ascii_case(f))
                        == Some(true)
                });
            }
            if let Some(t) = note_type {
                items.retain(|n| n.get("noteType").and_then(|v| v.as_str()) == Some(t));
            }
            items.truncate(limit(100));
            respond_json(request, 200, &list_envelope(Value::Array(items)));
        }
        (Method::Get, ["v1", "notes", "search"]) => {
            let q = query_get(params, "q").unwrap_or("");
            if q.trim().is_empty() {
                respond_error(request, 400, "validation_error", "Search query is required");
                return;
            }
            let results: Vec<Value> = crate::tools::search_notes(q, limit(20))
                .into_iter()
                .map(|(score, n)| {
                    let preview: String = n.content.chars().take(200).collect();
                    json!({
                        "id": n.id,
                        "title": n.title,
                        "folder": n.folder,
                        "score": score,
                        "preview": preview,
                        "updatedTs": n.updated_ts,
                    })
                })
                .collect();
            respond_json(request, 200, &list_envelope(json!(results)));
        }
        (Method::Get, ["v1", "notes", id]) => match parse_id(id).and_then(crate::notes::get) {
            Some(note) => respond_json(request, 200, &json!({ "data": note })),
            None => respond_error(request, 404, "not_found", &format!("Note {} not found", id)),
        },
        (Method::Post, ["v1", "notes", "create"]) => {
            let note = crate::notes::create(
                str_field(body, "title").unwrap_or("Untitled Note"),
                str_field(body, "content").unwrap_or(""),
                "api",
                str_field(body, "folder").unwrap_or(""),
            );
            let _ = app.emit("yap-notes-changed", ());
            respond_json(request, 201, &json!({ "data": note }));
        }
        (Method::Patch, ["v1", "notes", id]) => {
            let Some(id) = parse_id(id) else {
                respond_error(request, 404, "not_found", "Invalid note id");
                return;
            };
            let participants = body.get("participants").and_then(|v| v.as_array()).map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            });
            match crate::notes::update(
                id,
                str_field(body, "title").map(|s| s.to_string()),
                str_field(body, "content").map(|s| s.to_string()),
                str_field(body, "folder").map(|s| s.to_string()),
                participants,
            ) {
                Ok(()) => {
                    let _ = app.emit("yap-notes-changed", ());
                    let note = crate::notes::get(id);
                    respond_json(request, 200, &json!({ "data": note }));
                }
                Err(e) => respond_error(request, 404, "not_found", &e),
            }
        }
        (Method::Delete, ["v1", "notes", id]) => {
            match parse_id(id).and_then(crate::notes::get) {
                Some(note) => {
                    crate::notes::delete(note.id);
                    let _ = app.emit("yap-notes-changed", ());
                    respond_no_content(request);
                }
                None => {
                    respond_error(request, 404, "not_found", &format!("Note {} not found", id))
                }
            }
        }

        // ---- folders ----
        (Method::Get, ["v1", "folders", "list"]) => {
            respond_json(request, 200, &list_envelope(json!(crate::notes::folders())));
        }
        (Method::Post, ["v1", "folders", "create"]) => {
            let name = str_field(body, "name").unwrap_or("").trim().to_string();
            if name.is_empty() {
                respond_error(request, 400, "validation_error", "Folder name is required");
                return;
            }
            let folders = crate::notes::folder_create(&name);
            let _ = app.emit("yap-notes-changed", ());
            respond_json(request, 201, &json!({ "data": { "name": name, "folders": folders } }));
        }

        // ---- transcriptions (dictation history; id = unix-seconds ts) ----
        (Method::Get, ["v1", "transcriptions", "list"]) => {
            respond_json(request, 200, &list_envelope(crate::history::list(limit(50))));
        }
        (Method::Get, ["v1", "transcriptions", id]) => {
            match parse_id(id).and_then(crate::history::get_by_ts) {
                Some(e) => respond_json(request, 200, &json!({ "data": e })),
                None => respond_error(
                    request,
                    404,
                    "not_found",
                    &format!("Transcription {} not found", id),
                ),
            }
        }
        (Method::Delete, ["v1", "transcriptions", id]) => {
            match parse_id(id).map(crate::history::delete_by_ts) {
                Some(true) => respond_no_content(request),
                _ => respond_error(
                    request,
                    404,
                    "not_found",
                    &format!("Transcription {} not found", id),
                ),
            }
        }

        _ => respond_error(request, 404, "not_found", "Not found"),
    }
}

fn parse_id(s: &str) -> Option<u64> {
    let id = s.parse::<u64>().ok()?;
    (id > 0).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_decode_handles_percent_and_plus() {
        assert_eq!(url_decode("hello+world"), "hello world");
        assert_eq!(url_decode("a%20b%2Fc"), "a b/c");
        assert_eq!(url_decode("caf%C3%A9"), "café");
        // Malformed escapes pass through instead of panicking.
        assert_eq!(url_decode("100%"), "100%");
        assert_eq!(url_decode("%zz"), "%zz");
    }

    #[test]
    fn parse_query_splits_pairs() {
        let p = parse_query("q=hello+world&limit=5&flag");
        assert_eq!(query_get(&p, "q"), Some("hello world"));
        assert_eq!(query_get(&p, "limit"), Some("5"));
        assert_eq!(query_get(&p, "flag"), Some(""));
        assert_eq!(query_get(&p, "missing"), None);
    }

    #[test]
    fn token_compare_matches_only_exact() {
        assert!(token_matches("Bearer abc", "Bearer abc"));
        assert!(!token_matches("Bearer abd", "Bearer abc"));
        assert!(!token_matches("", "Bearer abc"));
    }

    #[test]
    fn parse_id_rejects_junk() {
        assert_eq!(parse_id("42"), Some(42));
        assert_eq!(parse_id("0"), None);
        assert_eq!(parse_id("abc"), None);
        assert_eq!(parse_id("-1"), None);
    }
}

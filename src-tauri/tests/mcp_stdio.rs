//! `yap.exe mcp` end to end: the real binary, started the way an AI app
//! starts it (stdin/stdout piped), talking to a fake running Yap: a
//! loopback server behind a discovery file, like the local API bridge.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TOKEN: &str = "integration-test-token";

/// A fake Yap: answers the bridge routes the MCP server uses, with one
/// meeting, and only with the right token.
fn fake_yap(dir: &Path) -> PathBuf {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let authed = req
                .headers()
                .iter()
                .any(|h| h.field.equiv("Authorization") && h.value.as_str() == format!("Bearer {TOKEN}"));
            let path = req.url().split('?').next().unwrap_or_default().to_string();
            let (status, body) = if !authed {
                (401, json!({ "error": { "code": "unauthorized", "message": "Unauthorized" } }))
            } else {
                match path.as_str() {
                    "/v1/mcp/config" => (200, json!({ "data": { "allowWrites": false } })),
                    "/v1/meetings/list" => (200, json!({ "data": [meeting_summary()], "has_more": false, "next_cursor": null })),
                    "/v1/notes/5" => (200, json!({ "data": meeting_note() })),
                    _ => (404, json!({ "error": { "code": "not_found", "message": "Not found" } })),
                }
            };
            let _ = req.respond(tiny_http::Response::from_string(body.to_string()).with_status_code(status));
        }
    });
    let file = dir.join("cli-bridge.json");
    std::fs::write(&file, json!({ "version": 1, "port": port, "token": TOKEN }).to_string()).unwrap();
    file
}

const T0: u64 = 1_759_660_200;

fn meeting_summary() -> Value {
    json!({
        "id": 5, "title": "Design review", "folder": "Meetings", "participants": ["Ana"],
        "createdTs": T0, "updatedTs": T0 + 900, "startTs": T0, "endTs": T0 + 840,
        "durationSecs": 840, "lines": 2, "pages": 1, "hasSummary": true, "hasNotes": false, "digests": 0
    })
}

fn meeting_note() -> Value {
    json!({
        "id": 5, "title": "Design review", "noteType": "meeting", "folder": "Meetings",
        "participants": ["Ana"], "source": "meeting",
        "enhancedContent": "## Action plan\n### Ana\n- [ ] ship the new onboarding (due: Friday)",
        "transcript": [
            { "source": "you", "text": "Can we ship the onboarding this week?", "ts": T0 },
            { "source": "them", "text": "Ana: yes, by Friday.", "ts": T0 + 840 }
        ],
        "createdTs": T0, "updatedTs": T0 + 900
    })
}

/// One `yap.exe mcp`, with a reader thread so a silent server fails the
/// test instead of hanging it.
struct Mcp {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    next_id: u64,
}

impl Mcp {
    fn spawn(bridge_file: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_yap"))
            .arg("mcp")
            .env("YAP_BRIDGE_FILE", bridge_file)
            .env_remove("YAP_E2E")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("yap.exe mcp starts");
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let stdin = child.stdin.take();
        Mcp { child, stdin, lines: rx, next_id: 1 }
    }

    fn send(&mut self, msg: Value) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(20))
                .unwrap_or_else(|_| panic!("no reply to {method}"));
            // stdout carries protocol messages only: every line is JSON-RPC.
            let msg: Value = serde_json::from_str(&line).unwrap_or_else(|_| panic!("not JSON on stdout: {line}"));
            assert_eq!(msg["jsonrpc"], "2.0", "{line}");
            if msg["id"] == json!(id) {
                return msg;
            }
        }
    }

    fn tool(&mut self, name: &str, args: Value) -> (bool, String) {
        let r = self.request("tools/call", json!({ "name": name, "arguments": args }));
        (
            r["result"]["isError"] == json!(true),
            r["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string(),
        )
    }

    /// Hang up like a client does (close stdin); the server must exit 0.
    fn hang_up(mut self) {
        drop(self.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "exit status {status}");
                return;
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                panic!("yap.exe mcp didn't exit after stdin closed");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yap-mcp-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_2025_client_lists_and_reads_meetings_over_stdio() {
    let dir = scratch("session");
    let mut mcp = Mcp::spawn(&fake_yap(&dir));
    let init = mcp.request(
        "initialize",
        json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "it", "version": "1" } }),
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "yap");
    mcp.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));

    let tools = mcp.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["list_meetings", "search_meetings", "get_meeting", "search_notes", "get_note", "list_folders"]);

    let (err, text) = mcp.tool("list_meetings", json!({}));
    assert!(!err, "{text}");
    assert!(text.contains("**Design review**") && text.contains("14 min") && text.contains("with Ana"), "{text}");

    let (err, text) = mcp.tool("get_meeting", json!({ "id": 5 }));
    assert!(!err, "{text}");
    assert!(text.contains("## AI summary\n## Action plan"), "{text}");
    assert!(text.contains("[14:00] Them: Ana: yes, by Friday."), "{text}");

    let (err, text) = mcp.tool("get_note", json!({ "id": 6 }));
    assert!(err && text.contains("no note with id 6"), "{text}");
    mcp.hang_up();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn with_yap_closed_the_tools_ask_for_yap() {
    let dir = scratch("closed");
    let mut mcp = Mcp::spawn(&dir.join("no-bridge-here.json"));
    mcp.request(
        "initialize",
        json!({ "protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": { "name": "it", "version": "1" } }),
    );
    mcp.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    let (err, text) = mcp.tool("search_meetings", json!({ "query": "onboarding" }));
    assert!(err);
    assert!(text.contains("Open Yap to let your AI read your notes"), "{text}");
    mcp.hang_up();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_2026_client_probes_with_server_discover() {
    let dir = scratch("modern");
    let mut mcp = Mcp::spawn(&fake_yap(&dir));
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": { "name": "it", "version": "1" },
        "io.modelcontextprotocol/clientCapabilities": {}
    });
    let d = mcp.request("server/discover", json!({ "_meta": meta }));
    let versions = d["result"]["supportedVersions"].as_array().unwrap();
    assert!(versions.contains(&json!("2026-07-28")) && versions.contains(&json!("2025-11-25")));
    assert_eq!(d["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"], "yap");
    let r = mcp.request(
        "tools/call",
        json!({ "_meta": meta, "name": "get_meeting", "arguments": { "id": 5 } }),
    );
    assert_eq!(r["result"]["resultType"], "complete");
    assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("# Design review"));
    mcp.hang_up();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_client_that_leaves_at_once_gets_a_clean_exit() {
    let dir = scratch("probe");
    Mcp::spawn(&dir.join("cli-bridge.json")).hang_up();
    let _ = std::fs::remove_dir_all(&dir);
}

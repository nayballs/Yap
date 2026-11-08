//! The endpoint over real HTTP on 127.0.0.1, with made-up reads (fictional
//! meetings, memory and calendar) and its events kept.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::approve::describe;
use super::reads::{render_calendar, Reads};
use super::server::{Emit, Endpoint};
use super::backup::{self, restore, Entry, Manifest, State, MAX_BACKUP_BYTES};
use super::*;
use crate::integrations::mcp::{list_meetings, BridgeError};
use crate::notes::Note;

// ---- the made-up Yap ----

fn meeting() -> Note {
    serde_json::from_value(json!({
        "id": 5,
        "title": "Planning sync",
        "noteType": "meeting",
        "folder": "Meetings",
        "participants": ["Alex", "Jordan"],
        "createdTs": 1_759_000_000u64,
        "updatedTs": 1_759_000_000u64,
        "transcript": [
            { "source": "you", "text": "Let's look at the launch plan.", "ts": 1_759_000_100u64 },
            { "source": "them", "text": "The beta goes out on Friday.", "ts": 1_759_000_115u64 }
        ]
    }))
    .unwrap()
}

struct Fake {
    notes: Vec<Note>,
}

impl Reads for Fake {
    fn bridge(&self, method: &str, path: &str) -> Result<Value, BridgeError> {
        assert_eq!(method, "GET", "a run only reads");
        let route = path.split('?').next().unwrap_or("");
        match route {
            "/v1/mcp/config" => Ok(json!({ "data": { "allowWrites": false } })),
            "/v1/meetings/list" => Ok(json!({ "data": list_meetings(&self.notes, None, 20) })),
            r if r.starts_with("/v1/notes/") => {
                let id: u64 = r.trim_start_matches("/v1/notes/").parse().unwrap_or(0);
                match self.notes.iter().find(|n| n.id == id) {
                    Some(n) => Ok(json!({ "data": n })),
                    None => Err(BridgeError::Api { status: 404, message: format!("Note {id} not found") }),
                }
            }
            _ => Err(BridgeError::Api { status: 404, message: "Not found".into() }),
        }
    }

    fn memory(&self, query: &str) -> Option<String> {
        query
            .to_lowercase()
            .contains("sam")
            .then(|| "From Yap's memory (what the user told Yap before; use it if it helps):\n- Sam (person): a colleague on the launch team.".to_string())
    }

    fn calendar_connected(&self) -> bool {
        true
    }

    fn calendar(&self, _args: &Value) -> Result<String, String> {
        render_calendar(&json!({
            "success": true,
            "data": { "asked": "tomorrow", "meetings": [{ "n": 1, "title": "Design review", "when": "Fri 6 Nov, 10:00–10:30" }] }
        }))
    }
}

type Events = Arc<Mutex<Vec<(String, Value)>>>;

fn endpoint(wait: Duration, max_backup: u64) -> (Arc<Endpoint>, Events) {
    let events: Events = Arc::default();
    let kept = Arc::clone(&events);
    let emit: Emit = Arc::new(move |name: &str, v: Value| kept.lock().unwrap().push((name.to_string(), v)));
    let ep = Endpoint::start_with(Arc::new(Fake { notes: vec![meeting()] }), emit, wait, max_backup).unwrap();
    (ep, events)
}

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yap-claude-live-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ---- a client, as Claude Code's ----

/// One HTTP request: `(status, body)`.
fn http(port: u16, method: &str, path: &str, headers: &[(&str, String)], body: &str) -> (u16, String) {
    let raw = http_raw(port, method, path, headers, body);
    let (status, body) = crate::integrations::mcp::parse_http_response(&raw).expect("an HTTP answer");
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// One HTTP request: the whole answer, head and all.
fn http_raw(port: u16, method: &str, path: &str, headers: &[(&str, String)], body: &str) -> Vec<u8> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let mut req = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
        req.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
    }
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    raw
}

/// A run's MCP client, from its own `--mcp-config`.
struct Client {
    port: u16,
    path: String,
    auth: String,
    next: u64,
}

impl Client {
    fn of(h: &RunHandle) -> Client {
        let server = &h.mcp_config()["mcpServers"]["yap"];
        let url = server["url"].as_str().unwrap();
        let path = url.split_once(&format!("127.0.0.1:{}", h.port())).unwrap().1.to_string();
        // The key as Claude Code sends it: its config names it, the run's
        // environment has it.
        assert_eq!(server["headers"]["Authorization"], "Bearer ${YAP_RUN_KEY}");
        Client { port: h.port(), path, auth: h.bearer(), next: 1 }
    }

    fn post(&self, msg: &Value) -> (u16, String) {
        http(self.port, "POST", &self.path, &[("Authorization", self.auth.clone()), ("Content-Type", "application/json".into()), ("Accept", "application/json, text/event-stream".into())], &msg.to_string())
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        let (status, body) = self.post(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        assert_eq!(status, 200, "{body}");
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["id"], json!(id));
        v
    }

    /// `(is_error, text)`.
    fn tool(&mut self, name: &str, args: Value) -> (bool, String) {
        let r = self.request("tools/call", json!({ "name": name, "arguments": args }));
        (r["result"]["isError"] == json!(true), r["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string())
    }
}

fn names(list: &Value) -> Vec<String> {
    list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()
}

// ---- MCP ----

#[test]
fn a_chat_run_connects_lists_and_reads_over_real_http() {
    let (ep, _) = endpoint(Duration::from_secs(5), MAX_BACKUP_BYTES);
    let h = ep.register(RunSpec::new("Plan chat", Tools::Chat)).unwrap();

    // The run's own config: Claude Code's http server entry, its key.
    let server = &h.mcp_config()["mcpServers"]["yap"];
    assert_eq!(server["type"], "http");
    assert_eq!(server["url"], format!("http://127.0.0.1:{}/mcp/{}", ep.port, h.id()));
    // The key is named, never written: Claude Code fills it in from the
    // run's environment (`YAP_RUN_KEY`).
    assert_eq!(server["headers"]["Authorization"], "Bearer ${YAP_RUN_KEY}");
    let env = h.env();
    assert_eq!(env.len(), 1);
    assert_eq!(env[0].0, crate::ai::claude_code::session::RUN_KEY_VAR);
    assert!(env[0].1.len() == 64 && env[0].1.bytes().all(|b| b.is_ascii_hexdigit()), "32 bytes of hex");
    assert!(!h.mcp_config().to_string().contains(&env[0].1));
    assert_eq!(h.bearer(), format!("Bearer {}", env[0].1));
    assert_eq!(server["timeout"], TOOL_TIMEOUT_MS);
    assert_eq!(h.settings(), json!({}), "no backups, no hook");

    let mut c = Client::of(&h);
    // Claude Code 2.1.296 probes the 2026 era first; "method not found"
    // sends it to initialize.
    let probe = c.request("server/discover", json!({ "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" } }));
    assert_eq!(probe["error"]["code"], -32601);
    let init = c.request(
        "initialize",
        json!({ "protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": { "name": "claude-code", "version": "2.1.296" } }),
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(init["result"]["serverInfo"]["name"], "yap");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    assert!(init["result"]["instructions"].as_str().unwrap().contains("never available"));
    // An older client gets its own version; one Yap doesn't know, Yap's.
    let old = c.request("initialize", json!({ "protocolVersion": "2025-06-18" }));
    assert_eq!(old["result"]["protocolVersion"], "2025-06-18");
    let future = c.request("initialize", json!({ "protocolVersion": "2099-01-01" }));
    assert_eq!(future["result"]["protocolVersion"], "2025-11-25");
    // A notification: 202, nothing back.
    let (status, body) = c.post(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    assert_eq!((status, body.as_str()), (202, ""));
    // No SSE stream to open.
    let (status, _) = http(c.port, "GET", &c.path, &[("Authorization", c.auth.clone()), ("Accept", "text/event-stream".into())], "");
    assert_eq!(status, 405);

    let list = c.request("tools/list", json!({}));
    assert_eq!(
        names(&list),
        ["list_meetings", "search_meetings", "get_meeting", "search_notes", "get_note", "list_tasks", "list_folders", "search_memory", "check_calendar"]
    );
    for t in list["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["annotations"]["readOnlyHint"], true, "{}", t["name"]);
        assert!(t["inputSchema"].is_object());
    }
    assert_eq!(
        h.allowed_tools().first().map(String::as_str),
        Some("mcp__yap__list_meetings"),
        "Claude Code's names for them, for --allowedTools"
    );
    assert!(h.allowed_tools().iter().all(|t| t.starts_with("mcp__yap__") && t != PERMISSION_TOOL));

    // The same views as `yap.exe mcp`.
    let (err, text) = c.tool("list_meetings", json!({}));
    assert!(!err, "{text}");
    assert!(text.starts_with("1 meeting in Yap") && text.contains("**Planning sync**") && text.contains("with Alex, Jordan"), "{text}");
    let (err, text) = c.tool("get_meeting", json!({ "id": 5 }));
    assert!(!err && text.contains("[0:15] Them: The beta goes out on Friday."), "{text}");
    let (err, text) = c.tool("get_note", json!({ "id": 404 }));
    assert!(err && text.contains("no note with id 404"));
    // Memory and the calendar.
    let (err, text) = c.tool("search_memory", json!({ "query": "Sam" }));
    assert!(!err && text.contains("Sam (person)"), "{text}");
    let (err, text) = c.tool("search_memory", json!({ "query": "the moon" }));
    assert!(!err && text.starts_with("Nothing in Yap's memory"));
    let (err, _) = c.tool("search_memory", json!({}));
    assert!(err);
    let (err, text) = c.tool("check_calendar", json!({ "day": "tomorrow" }));
    assert!(!err && text.contains("[1] Design review — Fri 6 Nov, 10:00–10:30"), "{text}");
    // Read only: no notes saved, no permission prompts, no other tools.
    for name in ["create_note", "approve", "get_dictations"] {
        let r = c.request("tools/call", json!({ "name": name, "arguments": {} }));
        assert_eq!(r["error"]["code"], -32602, "{name}");
    }
    assert_eq!(c.request("ping", json!({}))["result"], json!({}));
    assert_eq!(c.request("resources/list", json!({}))["error"]["code"], -32601);
    // A batch answers its requests only; junk is a parse error.
    let (status, body) = c.post(&json!([{ "jsonrpc": "2.0", "id": 90, "method": "ping" }, { "jsonrpc": "2.0", "method": "notifications/initialized" }]));
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!((status, v.as_array().map(Vec::len)), (200, Some(1)));
    let (status, body) = http(c.port, "POST", &c.path, &[("Authorization", c.auth.clone())], "{not json");
    assert_eq!(status, 400);
    assert!(body.contains("-32700"));
}

#[test]
fn a_code_chats_run_reads_meetings_notes_and_tasks_but_never_yaps_memory() {
    let (ep, _) = endpoint(Duration::from_secs(5), MAX_BACKUP_BYTES);
    let h = ep.register(RunSpec::new("Code chat", Tools::Chat).with_agents(true).with_memory(false)).unwrap();
    let mut c = Client::of(&h);
    let init = c.request("initialize", json!({ "protocolVersion": "2025-11-25" }));
    let said = init["result"]["instructions"].as_str().unwrap();
    assert!(!said.contains("search_memory") && !said.contains("remembers"), "{said}");
    assert!(said.contains("their meetings") && said.contains("check_calendar") && said.contains("start_agent"), "{said}");
    // Listed, allowed and run: everything but memory.
    let list = c.request("tools/list", json!({}));
    assert_eq!(
        names(&list),
        ["list_meetings", "search_meetings", "get_meeting", "search_notes", "get_note", "list_tasks", "list_folders", "check_calendar", "start_agent", "agent_status", "message_agent", "stop_agent"]
    );
    assert!(!h.allowed_tools().iter().any(|t| t.contains("memory")));
    assert_eq!(c.request("tools/call", json!({ "name": "search_memory", "arguments": { "query": "Sam" } }))["error"]["code"], -32602);
    let (err, text) = c.tool("get_meeting", json!({ "id": 5 }));
    assert!(!err && text.contains("[0:15] Them: The beta goes out on Friday."), "{text}");
    let (err, _) = c.tool("check_calendar", json!({ "day": "tomorrow" }));
    assert!(!err);
    // A plain chat's run reads memory, as before.
    let plain = ep.register(RunSpec::new("Plan chat", Tools::Chat)).unwrap();
    assert!(plain.allowed_tools().iter().any(|t| t == "mcp__yap__search_memory"));
}

#[test]
fn requests_without_the_run_s_key_or_from_another_site_are_refused() {
    let (ep, _) = endpoint(Duration::from_secs(5), MAX_BACKUP_BYTES);
    let h = ep.register(RunSpec::new("Plan chat", Tools::Chat)).unwrap();
    let other = ep.register(RunSpec::new("Another", Tools::Chat)).unwrap();
    let c = Client::of(&h);
    let ping = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }).to_string();
    let post = |path: &str, headers: &[(&str, String)]| http(c.port, "POST", path, headers, &ping).0;

    assert_eq!(post(&c.path, &[("Authorization", c.auth.clone())]), 200);
    assert_eq!(post(&c.path, &[]), 401, "no key");
    assert_eq!(post(&c.path, &[("Authorization", "Bearer 00".into())]), 401, "a wrong key");
    assert_eq!(post(&c.path, &[("Authorization", Client::of(&other).auth)]), 401, "another run's key");
    assert_eq!(post("/mcp/0123456789abcdef", &[("Authorization", c.auth.clone())]), 404, "no such run");
    assert_eq!(post("/v1/notes/list", &[("Authorization", c.auth.clone())]), 404, "not the Local API");
    // A web page on a name that resolves here, or one sending its origin.
    assert_eq!(post(&c.path, &[("Authorization", c.auth.clone()), ("Host", format!("rebind.example.com:{}", c.port))]), 403);
    assert_eq!(post(&c.path, &[("Authorization", c.auth.clone()), ("Origin", "https://app.example.com".into())]), 403);
    for headers in [vec![("Authorization", c.auth.clone())], vec![("Origin", "https://app.example.com".to_string())]] {
        let raw = String::from_utf8_lossy(&http_raw(c.port, "POST", &c.path, &headers, &ping)).to_lowercase();
        assert!(!raw.contains("access-control"), "no CORS: {raw}");
    }
    let (status, _) = http(c.port, "OPTIONS", &c.path, &[("Authorization", c.auth.clone()), ("Origin", "https://app.example.com".into())], "");
    assert_eq!(status, 403, "no preflight answered");
    // Ended: the run's URL is gone, the other run's still answers.
    let path = c.path.clone();
    drop(h);
    assert_eq!(post(&path, &[("Authorization", c.auth.clone())]), 404);
    let o = Client::of(&other);
    assert_eq!(http(o.port, "POST", &o.path, &[("Authorization", o.auth.clone())], &ping).0, 200);
}

// ---- approve ----

/// Call `approve` on its own thread (it waits for the person) and return
/// the decision Claude Code reads.
fn approving(c: &Client, input: Value) -> std::thread::JoinHandle<Value> {
    let (port, path, auth) = (c.port, c.path.clone(), c.auth.clone());
    std::thread::spawn(move || {
        let msg = json!({ "jsonrpc": "2.0", "id": "call-1", "method": "tools/call", "params": { "name": "approve", "arguments": input } });
        let (status, body) = http(port, "POST", &path, &[("Authorization", auth)], &msg.to_string());
        assert_eq!(status, 200, "{body}");
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["result"]["isError"], false, "{v}");
        serde_json::from_str(v["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    })
}

/// The card once it's waiting.
fn waiting(ep: &Endpoint) -> Pending {
    let start = Instant::now();
    loop {
        if let Some(p) = ep.approvals.pending().into_iter().next() {
            return p;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "a card in time");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn approve_asks_the_person_and_says_allow_deny_or_no_answer() {
    let folder = temp("approve");
    // Plenty of time to answer here (a loaded test run is slow); the
    // timeout has an endpoint of its own below.
    let (ep, events) = endpoint(Duration::from_secs(60), MAX_BACKUP_BYTES);
    let mut spec = RunSpec::new("Update the guide", Tools::Agent);
    spec.folder = Some(folder.clone());
    spec.approval = Some(Approval { run: "agent-7".into(), conversation: Some(42), origin: Origin::Voice, unattended: false });
    let h = ep.register(spec).unwrap();
    let mut c = Client::of(&h);
    assert_eq!(names(&c.request("tools/list", json!({}))), ["approve"], "an agent's only tool, for now");
    assert!(h.allowed_tools().is_empty());

    // Allowed: the input goes back as it was asked.
    let call = approving(&c, json!({ "tool_name": "Bash", "input": { "command": "npm test", "description": "Run the tests" }, "tool_use_id": "toolu_01" }));
    let card = waiting(&ep);
    assert_eq!(
        (card.run.as_str(), card.conversation, card.origin, card.tool.as_str(), card.summary.as_str(), card.detail.as_str(), card.label.as_str()),
        ("agent-7", Some(42), Origin::Voice, "Bash", "Run a command: npm test", "npm test", "Update the guide")
    );
    let sent = events.lock().unwrap().iter().find(|(n, _)| n == "yap-claude-approve").map(|(_, v)| v.clone()).unwrap();
    assert_eq!((sent["id"].as_u64(), sent["run"].as_str(), sent["origin"].as_str(), sent["summary"].as_str()), (Some(card.id), Some("agent-7"), Some("voice"), Some("Run a command: npm test")));
    assert!(ep.approvals.answer(card.id, true));
    assert!(!ep.approvals.answer(card.id, false), "answered once");
    let decision = call.join().unwrap();
    assert_eq!(decision, json!({ "behavior": "allow", "updatedInput": { "command": "npm test", "description": "Run the tests" } }));
    assert!(ep.approvals.pending().is_empty());

    // Denied.
    let call = approving(&c, json!({ "tool_name": "Write", "input": { "file_path": folder.join("notes.md"), "content": "# Plan" } }));
    let card = waiting(&ep);
    assert_eq!(card.summary, "Write file: notes.md", "named in the agent's folder");
    assert!(ep.approvals.answer(card.id, false));
    let decision = call.join().unwrap();
    assert_eq!(decision["behavior"], "deny");
    assert!(decision["message"].as_str().unwrap().contains("said no"));

    let outcomes = |events: &Events| -> Vec<String> {
        events.lock().unwrap().iter().filter(|(n, _)| n == "yap-claude-approve-done").map(|(_, v)| v["outcome"].as_str().unwrap().to_string()).collect()
    };
    assert_eq!(outcomes(&events), ["allowed", "denied"]);

    // Nobody answers: a no, said so.
    let (quick, quick_events) = endpoint(Duration::from_millis(300), MAX_BACKUP_BYTES);
    let unanswered = quick.register(RunSpec::new("Unattended", Tools::Agent)).unwrap();
    let call = approving(&Client::of(&unanswered), json!({ "tool_name": "Bash", "input": { "command": "git push" } }));
    let decision = call.join().unwrap();
    assert_eq!(decision["behavior"], "deny");
    let message = decision["message"].as_str().unwrap();
    assert!(message.contains("Nobody answered") && message.contains("within 1 second"), "{decision}");
    let card = quick_events.lock().unwrap().iter().find(|(n, _)| n == "yap-claude-approve").map(|(_, v)| v["id"].as_u64().unwrap()).unwrap();
    assert!(!quick.approvals.answer(card, true), "too late");
    assert!(quick.approvals.pending().is_empty());
    assert_eq!(outcomes(&quick_events), ["timedOut"]);

    // A question isn't a permission: Claude decides, no card.
    let before = events.lock().unwrap().len();
    let decision = approving(&c, json!({ "tool_name": "AskUserQuestion", "input": { "questions": [] } })).join().unwrap();
    assert_eq!(decision["behavior"], "deny");
    assert_eq!(events.lock().unwrap().len(), before);
    // Not a permission prompt at all: a tool error.
    let (err, text) = c.tool("approve", json!({}));
    assert!(err && text.contains("tool_name"));
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn in_bypass_permissions_a_prompt_is_answered_no_at_once_with_no_card() {
    let (ep, events) = endpoint(Duration::from_secs(60), MAX_BACKUP_BYTES);
    let mut spec = RunSpec::new("Catch up on the add-on", Tools::Agent);
    spec.approval = Some(Approval { run: "agent-8".into(), conversation: Some(43), origin: Origin::Voice, unattended: true });
    let h = ep.register(spec).unwrap();
    let c = Client::of(&h);
    // A safety check Claude Code still asks about in Bypass: no card, no wait, a no.
    let started = Instant::now();
    let decision = approving(&c, json!({ "tool_name": "Write", "input": { "file_path": ".claude/settings.json", "content": "{}" } })).join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(10), "never a wait for a card");
    assert_eq!(decision["behavior"], "deny");
    assert_eq!(decision["message"], super::approve::UNATTENDED);
    assert!(ep.approvals.pending().is_empty());
    assert!(!events.lock().unwrap().iter().any(|(n, _)| n == "yap-claude-approve" || n == "yap-claude-approve-done"), "no card at all");
}

#[test]
fn a_run_that_ends_or_a_cancelled_call_denies_the_prompt_waiting() {
    let (ep, events) = endpoint(Duration::from_secs(30), MAX_BACKUP_BYTES);
    // Cancelled by Claude Code (an interrupt).
    let h = ep.register(RunSpec::new("Tidy", Tools::Agent)).unwrap();
    let c = Client::of(&h);
    let call = approving(&c, json!({ "tool_name": "Bash", "input": { "command": "npm run build" } }));
    let card = waiting(&ep);
    assert_eq!(card.run, h.id(), "no agent id given: the run's own");
    let (status, _) = c.post(&json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": "call-1", "reason": "interrupted" } }));
    assert_eq!(status, 202);
    let decision = call.join().unwrap();
    assert_eq!(decision["behavior"], "deny");
    assert!(decision["message"].as_str().unwrap().contains("cancelled"));

    // The run ends (Stop): its prompt is denied at once.
    let call = approving(&c, json!({ "tool_name": "Edit", "input": { "file_path": "/srv/app/main.rs", "old_string": "a", "new_string": "b" } }));
    waiting(&ep);
    drop(h);
    let decision = call.join().unwrap();
    assert!(decision["message"].as_str().unwrap().contains("stopped"), "{decision}");
    assert!(ep.approvals.pending().is_empty());
    let outcomes: Vec<String> = events.lock().unwrap().iter().filter(|(n, _)| n == "yap-claude-approve-done").map(|(_, v)| v["outcome"].as_str().unwrap().to_string()).collect();
    assert_eq!(outcomes, ["cancelled", "ended"]);
}

#[test]
fn cards_say_what_a_tool_would_do_in_plain_words() {
    let folder = std::env::temp_dir().join("yap-describe");
    let rel = |p: &str| Path::new(p).display().to_string();
    let file = folder.join("src").join("app.ts");
    let (s, d) = describe("Edit", &json!({ "file_path": file, "old_string": "let a = 1;", "new_string": "let a = 2;" }), Some(&folder));
    assert_eq!(s, format!("Edit file: {}", Path::new("src").join("app.ts").display()));
    assert_eq!(d, "Replace:\nlet a = 1;\n\nWith:\nlet a = 2;");
    let (s, _) = describe("Edit", &json!({ "file_path": "/elsewhere/app.ts" }), Some(&folder));
    assert_eq!(s, format!("Edit file: {}", rel("/elsewhere/app.ts")), "outside the folder: the whole path");
    let (s, d) = describe("Bash", &json!({ "command": format!("echo {}\nsecond line", "x".repeat(200)) }), None);
    assert!(s.starts_with("Run a command: echo xxx") && s.chars().count() <= "Run a command: ".len() + 80 && s.ends_with('…'));
    assert!(d.contains('\n'), "the detail keeps the lines");
    // A cd into the run's folder: gone from the line, kept in the detail; never the model's description.
    let garden = Path::new(r"D:\Projects\garden-planner");
    let cmd = r#"cd "/d/Projects/garden-planner" && rm -rf "/d/Projects/garden-planner/build""#;
    let (s, d) = describe("Bash", &json!({ "command": cmd, "description": "Tidy up" }), Some(garden));
    assert_eq!((s.as_str(), d.as_str()), ("Run a command: rm -rf build", cmd));
    assert_eq!(describe("WebFetch", &json!({ "url": "https://docs.example.com/guide?x=1" }), None).0, "Open a web page: docs.example.com");
    assert_eq!(describe("WebSearch", &json!({ "query": "rust  tiny_http" }), None).0, "Search the web: rust tiny_http");
    assert_eq!(describe("mcp__tracker__create_issue", &json!({ "title": "Bug" }), None).0, "Use create_issue from tracker");
    assert_eq!(describe("MultiEdit", &json!({ "file_path": "/srv/a.rs", "edits": [{ "old_string": "a", "new_string": "b" }, {}] }), None).1, "2 changes. The first:\nReplace:\na\n\nWith:\nb");
    assert_eq!(describe("Frobnicate", &json!({}), None), ("Use Frobnicate".to_string(), String::new()));
    let long = "y".repeat(5_000);
    assert_eq!(describe("Write", &json!({ "file_path": "/srv/a.txt", "content": long }), None).1.chars().count(), 2_000, "trimmed");
}

// ---- the backup hook and Undo ----

fn hook(h: &RunHandle, input: &Value) -> (u16, String) {
    let hook = &h.settings()["hooks"]["PreToolUse"][0]["hooks"][0];
    let url = hook["url"].as_str().unwrap();
    let path = url.split_once(&format!("127.0.0.1:{}", h.port())).unwrap().1;
    http(h.port(), "POST", path, &[("Authorization", h.bearer()), ("Content-Type", "application/json".into())], &input.to_string())
}

fn edit_input(work: &Path, tool: &str, key: &str, path: &str) -> Value {
    json!({
        "session_id": "session-1",
        "cwd": work,
        "permission_mode": "auto",
        "hook_event_name": "PreToolUse",
        "tool_name": tool,
        "tool_input": { key: path, "old_string": "x", "new_string": "y", "content": "z" },
        "tool_use_id": "toolu_02"
    })
}

#[test]
fn the_backup_hook_keeps_the_first_copy_and_undo_puts_everything_back() {
    let root = temp("backup");
    let work = root.join("work");
    let backups = root.join("agent-backup");
    std::fs::create_dir_all(work.join("docs")).unwrap();
    std::fs::write(work.join("a.txt"), "before").unwrap();
    std::fs::write(work.join("docs").join("guide.ipynb"), "{\"cells\":[]}").unwrap();
    std::fs::write(work.join("big.bin"), vec![7u8; 2048]).unwrap();
    let (ep, _) = endpoint(Duration::from_secs(5), 1024);
    let mut spec = RunSpec::new("Update the guide", Tools::Agent);
    spec.backup_dir = Some(backups.clone());
    spec.folder = Some(work.clone());
    let h = ep.register(spec).unwrap();

    // The run's settings: the http hook, before edits, with its key.
    let settings = h.settings();
    let group = &settings["hooks"]["PreToolUse"][0];
    assert_eq!(group["matcher"], "Edit|Write|MultiEdit|NotebookEdit");
    let hk = &group["hooks"][0];
    assert_eq!((hk["type"].as_str(), hk["timeout"].as_u64()), (Some("http"), Some(HOOK_TIMEOUT_SECS)));
    assert_eq!(hk["url"], format!("http://127.0.0.1:{}/hook/{}/PreToolUse", h.port(), h.id()));
    // Its key named, as the MCP server's is; an http hook's headers take
    // only the variables it lists.
    assert_eq!(hk["headers"]["Authorization"], "Bearer ${YAP_RUN_KEY}");
    assert_eq!(hk["allowedEnvVars"], json!(["YAP_RUN_KEY"]));
    assert!(!settings.to_string().contains(&h.env()[0].1), "the key is never in the settings");

    let a = work.join("a.txt");
    // First edit: copied, and the hook says carry on.
    assert_eq!(hook(&h, &edit_input(&work, "Edit", "file_path", a.to_str().unwrap())), (200, "{}".to_string()));
    std::fs::write(&a, "after one edit").unwrap();
    // A second edit of it keeps the first copy.
    assert_eq!(hook(&h, &edit_input(&work, "MultiEdit", "file_path", a.to_str().unwrap())).0, 200);
    // A new file, named relative to the session's cwd: created.
    assert_eq!(hook(&h, &edit_input(&work, "Write", "file_path", "new.txt")).0, 200);
    std::fs::write(work.join("new.txt"), "made by the agent").unwrap();
    // A notebook, and a file too big to copy.
    assert_eq!(hook(&h, &edit_input(&work, "NotebookEdit", "notebook_path", work.join("docs").join("guide.ipynb").to_str().unwrap())).0, 200);
    assert_eq!(hook(&h, &edit_input(&work, "Write", "file_path", work.join("big.bin").to_str().unwrap())).0, 200);
    // Not an edit, the backup folder itself, a wrong key, another event: nothing.
    assert_eq!(hook(&h, &edit_input(&work, "Bash", "file_path", a.to_str().unwrap())).0, 200);
    assert_eq!(hook(&h, &edit_input(&work, "Write", "file_path", backups.join("manifest.json").to_str().unwrap())).0, 200);
    let wrong = http(h.port(), "POST", &format!("/hook/{}/PreToolUse", h.id()), &[("Authorization", "Bearer 00".into())], &edit_input(&work, "Edit", "file_path", "other.txt").to_string());
    assert_eq!(wrong.0, 401);
    let post = http(h.port(), "POST", &format!("/hook/{}/PostToolUse", h.id()), &[("Authorization", Client::of(&h).auth)], &edit_input(&work, "Edit", "file_path", "other.txt").to_string());
    assert_eq!(post, (200, "{}".to_string()));

    let m = backup::read_manifest(&backups).unwrap();
    let states: Vec<(String, State)> = m.files.iter().map(|e| (e.original.file_name().unwrap().to_string_lossy().into_owned(), e.state)).collect();
    assert_eq!(
        states,
        [("a.txt".into(), State::Copied), ("new.txt".into(), State::Created), ("guide.ipynb".into(), State::Copied), ("big.bin".into(), State::TooBig)]
    );
    assert_eq!(m.files[0].original, a, "the absolute path");
    assert_eq!(std::fs::read_to_string(backups.join(&m.files[0].backup)).unwrap(), "before", "the first copy wins");
    assert!(m.files.iter().all(|e| e.state != State::Copied || !e.backup.contains(['/', '\\'])));
    std::fs::write(work.join("docs").join("guide.ipynb"), "{\"cells\":[1]}").unwrap();

    // Undo all.
    let report = restore(&backups);
    assert_eq!(report.restored, [a.clone(), work.join("docs").join("guide.ipynb")]);
    assert_eq!(report.deleted, [work.join("new.txt")]);
    assert_eq!(report.skipped.len(), 1, "the big file was never copied");
    assert!(report.refused.is_empty() && report.failed.is_empty() && report.error.is_none(), "{report:?}");
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "before");
    assert_eq!(std::fs::read_to_string(work.join("docs").join("guide.ipynb")).unwrap(), "{\"cells\":[]}");
    assert!(!work.join("new.txt").exists());
    // Undo again: nothing left to delete.
    let again = restore(&backups);
    assert_eq!(again.skipped.iter().filter(|s| s.reason == "already gone").count(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn undo_refuses_what_a_tampered_manifest_points_at() {
    let root = temp("tampered");
    let backups = root.join("backup");
    std::fs::create_dir_all(&backups).unwrap();
    let victim = root.join("victim.txt");
    std::fs::write(&victim, "keep me").unwrap();
    std::fs::write(root.join("secret.txt"), "not a backup").unwrap();
    let entry = |original: PathBuf, state: State, backup: &str| Entry { original, state, backup: backup.into(), bytes: 1, tool: "Edit".into(), ts: 0, note: String::new() };
    let m = Manifest {
        version: 1,
        files: vec![
            entry(victim.clone(), State::Copied, "../secret.txt"),
            entry(PathBuf::from("relative.txt"), State::Created, ""),
            entry(root.join("x").join("..").join("victim.txt"), State::Created, ""),
            entry(backups.join("manifest.json"), State::Created, ""),
        ],
    };
    std::fs::write(backups.join("manifest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
    let report = restore(&backups);
    assert_eq!(report.refused.len(), 4, "{report:?}");
    assert!(report.restored.is_empty() && report.deleted.is_empty());
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep me");
    assert!(backups.join("manifest.json").exists());
    // A manifest that isn't one: nothing done, said.
    std::fs::write(backups.join("manifest.json"), "garbage").unwrap();
    assert!(restore(&backups).error.is_some());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn edits_at_once_back_a_file_up_once() {
    let root = temp("parallel");
    let backups = root.join("backup");
    let file = root.join("shared.txt");
    std::fs::write(&file, "original").unwrap();
    let (ep, _) = endpoint(Duration::from_secs(5), MAX_BACKUP_BYTES);
    let mut spec = RunSpec::new("Parallel", Tools::Agent);
    spec.backup_dir = Some(backups.clone());
    let h = Arc::new(ep.register(spec).unwrap());
    let threads: Vec<_> = (0..6)
        .map(|_| {
            let (h, root, file) = (Arc::clone(&h), root.clone(), file.clone());
            std::thread::spawn(move || hook(&h, &edit_input(&root, "Edit", "file_path", file.to_str().unwrap())).0)
        })
        .collect();
    for t in threads {
        assert_eq!(t.join().unwrap(), 200);
    }
    assert_eq!(backup::read_manifest(&backups).unwrap().files.len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

/// **The real Claude Code against the endpoint** (opt in: `YAP_TEST_CLAUDE_EXE`
/// = the `claude` executable; `cargo test -- --ignored real_claude_code`).
/// It runs with a clean environment and an empty config folder, so it has
/// no sign-in and never reaches a model: the run stops at "Not logged in",
/// after connecting its MCP servers. What it checks: Claude Code's probe,
/// `initialize` and `tools/list` against this endpoint (its `system/init`
/// says the server is connected and lists its tools), that it takes the
/// hook settings, and that `--permission-prompt-tool mcp__yap__approve`
/// keeps `approve` out of the model's tools (2.1.296: listed without the
/// flag, gone with it), so the model can't call it itself.
#[test]
#[ignore = "runs the person's own Claude Code CLI (YAP_TEST_CLAUDE_EXE)"]
fn real_claude_code_connects_to_the_endpoint() {
    let Some(exe) = std::env::var_os("YAP_TEST_CLAUDE_EXE").filter(|v| !v.is_empty()) else {
        eprintln!("YAP_TEST_CLAUDE_EXE isn't set: skipped");
        return;
    };
    let root = temp("real-claude");
    let (ep, _) = endpoint(Duration::from_secs(5), MAX_BACKUP_BYTES);
    for (tools, prompt_tool) in [(Tools::Chat, false), (Tools::Agent, false), (Tools::Agent, true)] {
        let mut spec = RunSpec::new("Real CLI", tools);
        if tools == Tools::Agent {
            spec.backup_dir = Some(root.join("backup"));
            spec.folder = Some(root.clone());
        }
        let h = ep.register(spec).unwrap();
        let mut cmd = std::process::Command::new(&exe);
        cmd.env_clear();
        for k in ["PATH", "SystemRoot", "SYSTEMROOT", "USERPROFILE", "HOME", "APPDATA", "LOCALAPPDATA", "TEMP", "TMP"] {
            if let Some(v) = std::env::var_os(k) {
                cmd.env(k, v);
            }
        }
        cmd.env("CLAUDE_CONFIG_DIR", root.join("config"))
            .envs(h.env())
            .current_dir(&root)
            .args(["-p", "hello", "--output-format", "stream-json", "--verbose", "--tools", "", "--strict-mcp-config", "--no-session-persistence"])
            .arg("--mcp-config")
            .arg(h.mcp_config().to_string())
            .arg("--settings")
            .arg(h.settings().to_string());
        if prompt_tool {
            cmd.args(["--permission-prompt-tool", PERMISSION_TOOL]);
        }
        let out = cmd.output().expect("Claude Code runs");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let init: Value = stdout
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .find(|v| v["type"] == "system" && v["subtype"] == "init")
            .unwrap_or_else(|| panic!("no init event: {stdout} {}", String::from_utf8_lossy(&out.stderr)));
        let yap = init["mcp_servers"].as_array().unwrap().iter().find(|s| s["name"] == "yap").cloned().unwrap();
        assert_eq!(yap["status"], "connected", "{init}");
        assert!(init.get("mcp_server_errors").is_none(), "{init}");
        let listed: Vec<&str> = init["tools"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        match (tools, prompt_tool) {
            (Tools::Chat, _) => assert!(listed.contains(&"mcp__yap__list_meetings"), "{listed:?}"),
            (Tools::Agent, false) => assert!(listed.contains(&PERMISSION_TOOL), "listed: {listed:?}"),
            (Tools::Agent, true) => assert!(!listed.contains(&PERMISSION_TOOL), "the model never sees it: {listed:?}"),
        }
        eprintln!("{tools:?} (prompt tool {prompt_tool}): Claude Code {} connected; tools {listed:?}", init["claude_code_version"]);
    }
    let _ = std::fs::remove_dir_all(&root);
}

// ---- Yap Code's tools on a plan chat's run ----

#[test]
fn a_chat_run_offered_agents_lists_and_allows_their_tools_and_runs_them_for_its_turn() {
    let (ep, _) = endpoint(Duration::from_secs(5), MAX_BACKUP_BYTES);
    let plain = ep.register(RunSpec::new("Plan chat", Tools::Chat)).unwrap();
    let h = ep.register(RunSpec::new("Plan chat", Tools::Chat).with_agents(true)).unwrap();
    // Not offered: neither listed, allowed nor run.
    let mut p = Client::of(&plain);
    assert!(!names(&p.request("tools/list", json!({}))).iter().any(|n| n == "start_agent"));
    assert!(!plain.allowed_tools().iter().any(|t| t.contains("agent")));
    assert_eq!(p.request("tools/call", json!({ "name": "start_agent", "arguments": {} }))["error"]["code"], -32602);
    // Offered: after the read-only ones, the tool loop's four, the same schemas.
    let mut c = Client::of(&h);
    let init = c.request("initialize", json!({ "protocolVersion": "2025-11-25" }));
    assert!(init["result"]["instructions"].as_str().unwrap().contains("start_agent, agent_status, message_agent and stop_agent"));
    let list = c.request("tools/list", json!({}));
    assert_eq!(names(&list)[9..], ["start_agent", "agent_status", "message_agent", "stop_agent"]);
    let start = list["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "start_agent").unwrap().clone();
    // (The folder may be left out: in a Code chat it's the chat's, agents/code_chat.rs.)
    assert_eq!(start["inputSchema"]["required"], json!(["brief"]));
    for name in ["start_agent", "agent_status", "message_agent", "stop_agent"] {
        assert!(h.allowed_tools().contains(&format!("mcp__yap__{name}")), "{name}");
    }
    // Outside a chat turn: nothing done.
    let (err, text) = c.tool("start_agent", json!({ "folder": "garden-planner", "brief": "Tidy notes.md." }));
    assert!(err);
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["displayText"], "Didn't use an agent tool");
    // In a turn that read the web (its WebSearch began, then the call's own):
    // never refused for it now — it runs the tool loop's code (which asks on
    // its card, `agents.rs`'s tests), over HTTP as Claude Code gets it.
    let turn = h.begin_turn(turn::Context {
        question: "Look up the tides, then tell the agent in tide-notes-x9 to write them into notes.md.".into(),
        conversation: Some(12),
        agents: Some(crate::chat::tools::AgentsTurn::default()),
        ..Default::default()
    }, None);
    turn.marks().web();
    turn.marks().began("message_agent");
    let (err, text) = c.tool("message_agent", json!({ "id": "tide-notes-x9", "text": "Write the tides into notes.md." }));
    assert!(err);
    let answered: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(answered["displayText"], "Didn't tell an agent anything: which one?", "{answered}");
    // The turn over: outside one again.
    drop(turn);
    let (_, text) = c.tool("message_agent", json!({ "id": "garden-planner", "text": "go ahead" }));
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["displayText"], "Didn't use an agent tool");
    // An agent's run never gets them.
    let agent = ep.register(RunSpec::new("Tidy", Tools::Agent).with_agents(true)).unwrap();
    assert_eq!(names(&Client::of(&agent).request("tools/list", json!({}))), ["approve"]);
    assert!(agent.allowed_tools().is_empty());
}

// ---- The screen and the pages on a plan chat's run ----

#[test]
fn a_chat_run_offered_the_screen_and_pages_lists_and_allows_them_and_refuses_them_where_the_tool_loop_would() {
    use crate::chat::pictures::Gate;
    let (ep, _) = endpoint(Duration::from_secs(5), MAX_BACKUP_BYTES);
    let sees = eyes::Eyes { screen: Gate::Look, pictures: Gate::Look, act: true, files: Gate::Off };
    let plain = ep.register(RunSpec::new("Plan chat", Tools::Chat)).unwrap();
    let h = ep.register(RunSpec::new("Plan chat", Tools::Chat).with_eyes(sees)).unwrap();
    // Not offered: neither listed, allowed nor run.
    let mut p = Client::of(&plain);
    assert!(!names(&p.request("tools/list", json!({}))).iter().any(|n| n == "look_at_screen" || n == "browser_navigate"));
    assert!(!plain.allowed_tools().iter().any(|t| t.ends_with("look_at_screen")));
    assert_eq!(p.request("tools/call", json!({ "name": "look_at_screen", "arguments": {} }))["error"]["code"], -32602);
    // Offered: after the read-only ones, the tool loop's own, the same
    // schemas; allowed by Claude Code's names; the model told of them.
    let mut c = Client::of(&h);
    let init = c.request("initialize", json!({ "protocolVersion": "2025-11-25" }));
    assert!(init["result"]["instructions"].as_str().unwrap().contains("look_at_screen takes a screenshot of their monitors"));
    let list = c.request("tools/list", json!({}));
    assert_eq!(names(&list)[9..], eyes::all_names());
    let look = list["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "look_at_screen").unwrap().clone();
    assert_eq!(look["inputSchema"], crate::chat::see_screen::definitions(Gate::Look, crate::chat::see_screen::Takes::Screen)[0]["function"]["parameters"]);
    for name in eyes::all_names() {
        assert!(h.allowed_tools().contains(&format!("mcp__yap__{name}")), "{name}");
    }
    // Outside a chat turn: nothing done.
    let (err, text) = c.tool("look_at_screen", json!({}));
    assert!(err);
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["displayText"], "Didn't look: not part of a chat turn");
    // A turn whose screen is off (a phone's, or from another app with no look allowed): refused, nothing taken.
    let turn = h.begin_turn(turn::Context { question: "Have a look at my screen".into(), conversation: Some(14), ..Default::default() }, None);
    let r = c.request("tools/call", json!({ "name": "look_at_screen", "arguments": {} }));
    assert_eq!(r["result"]["isError"], true);
    assert_eq!(r["result"]["content"].as_array().unwrap().len(), 1, "no pictures");
    assert_eq!(serde_json::from_str::<Value>(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap()["displayText"], "Didn't look at your screen: not for this question");
    let (err, text) = c.tool("browser_navigate", json!({ "url": "https://wiki.example.com/" }));
    assert!(err && text.contains("no panel beside the chat"), "{text}");
    drop(turn);
    // The plan is a cloud model: the screen goes only once "…with cloud
    // models too" is on (see_screen::execute asks once until then).
    let leaves = crate::chat::assistant::screen::leaves_pc(crate::ai::claude_plan::PROVIDER, crate::ai::claude_plan::BASE_URL);
    assert!(leaves);
    assert_eq!(crate::chat::assistant::screen::allowed(true, false, leaves), crate::chat::assistant::screen::Allowed::NotToTheCloud);
    // An agent's run never gets them.
    let agent = ep.register(RunSpec::new("Tidy", Tools::Agent).with_eyes(sees)).unwrap();
    assert_eq!(names(&Client::of(&agent).request("tools/list", json!({}))), ["approve"]);
    assert!(agent.allowed_tools().is_empty());
}

// ---- Yap's own endpoint ----

#[test]
fn yap_s_endpoint_starts_once_and_each_run_has_its_own_key() {
    let a = register_run(RunSpec::new("One", Tools::Chat)).unwrap();
    let b = register_run(RunSpec::new("Two", Tools::Agent)).unwrap();
    assert_eq!(a.port(), b.port(), "one listener");
    assert_ne!(a.id(), b.id());
    assert_ne!(a.env(), b.env());
    assert_eq!(a.mcp_config()["mcpServers"]["yap"]["headers"], b.mcp_config()["mcpServers"]["yap"]["headers"], "both name it; each run's environment has its own");
    assert!(!format!("{a:?}").contains(&Client::of(&a).auth[7..]), "Debug never shows the key");
    assert!(!claude_approve_answer(987_654, true), "nothing waits");
    assert!(claude_approve_pending().is_empty());
}

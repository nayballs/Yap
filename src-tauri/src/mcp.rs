//! Yap's MCP server: AI apps (Claude, the ChatGPT desktop app, Cursor, Gemini
//! CLI, VS Code…) read the person's meeting notes and notes, on this PC. The
//! local take on Wispr Flow's Notetaker MCP ("Sync Notetaker with your
//! favorite AI apps… does not have access to your dictations"), with no
//! cloud in between: the AI app talks to Yap, Yap reads its own store.
//!
//! **Shape.** An AI app launches `yap.exe mcp` (main.rs sends that argument
//! here before any of the app starts: no window, tray, hook or second
//! bridge) and speaks JSON-RPC over the child's stdin/stdout. This process
//! holds no data of its own. Every tool call goes to the *running* Yap
//! through the local API bridge (`bridge.rs`: `~/.yap/cli-bridge.json` gives
//! the port and token, re-read on every call), so `notes.json` keeps a single
//! writer. With Yap closed, or its Local API off, the tools say so.
//!
//! **Tools.** Meetings and notes only, never the dictation history (Wispr's
//! promise too): `list_meetings`, `search_meetings`, `get_meeting`,
//! `search_notes`, `get_note`, `list_folders`, plus `create_note` when the
//! person allows it (Settings → MCP, `config.mcp_allow_writes`).
//! Results are markdown for the model to read; a long transcript comes in
//! pages of about [`PAGE_TOKENS`] so it fits any client's context.
//!
//! **Protocol.** The official Rust SDK (`rmcp`), which serves both eras:
//! the 2025 `initialize` handshake (every client in use today) and
//! 2026-07-28's per-request `_meta` with `server/discover`.
//!
//! The meeting views the bridge serves for these tools (`/v1/meetings/*`)
//! live here too, so search results and `get_meeting` agree on page numbers.
//! Clients register the server through `mcp_clients.rs`.

use std::io::{Read, Write};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock,
    Implementation, JsonObject, ListToolsResult, PaginatedRequestParams, ProtocolVersion,
    ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use serde_json::{json, Value};

use crate::meeting_summary::{clock, estimate_tokens, render_digests};
use crate::notes::{Note, TranscriptSegment};

/// Transcript tokens per `get_meeting` page: about 20 minutes of talk, small
/// enough for any client's context next to the rest of a conversation.
pub const PAGE_TOKENS: usize = 6_000;
/// Budget for the rolling AI notes shown when a meeting has no summary yet.
const DIGEST_TOKENS: usize = 1_500;
/// Matching transcript lines shown per meeting in `search_meetings`.
const MAX_MATCHES: usize = 3;

/// What an AI app reads when Yap isn't there to ask.
pub const NOT_RUNNING: &str = "Yap isn't open, so your notes can't be read right now. Open Yap to let your AI read your notes (and check that Integrations → Local API is switched on in Yap).";

const INSTRUCTIONS: &str = "Yap is the user's voice-dictation and meeting-notes app on this computer. These tools read their Yap meetings (AI summaries, notes typed during the meeting and You/Them transcripts) and their other Yap notes, through the Yap app running on this PC. Their dictation history is never available here. Find a meeting with list_meetings (by date) or search_meetings (by topic, name or words said), then read it with get_meeting; long transcripts come in pages. In a transcript 'You' is the user and 'Them' is everyone else on the call, not told apart. Times are in the user's local time zone. If a tool says Yap isn't open, ask the user to open Yap.";

// ---- the meetings, as the bridge serves them (runs inside the app) ----

/// A meeting: a note that has been (or is being) recorded.
pub fn is_meeting(n: &Note) -> bool {
    n.note_type == "meeting" || !n.transcript.is_empty()
}

/// The meeting's zero: its first transcript line (else when the note was
/// made). Line times are counted from here, like the Notes view does.
fn t0(n: &Note) -> u64 {
    n.transcript.first().map(|s| s.ts).unwrap_or(n.created_ts)
}

fn speaker(seg: &TranscriptSegment) -> &'static str {
    if seg.source == "you" {
        "You"
    } else {
        "Them"
    }
}

/// One transcript line as the tools show it: `[12:34] Them: …`.
fn line(seg: &TranscriptSegment, t0: u64) -> String {
    format!(
        "[{}] {}: {}",
        clock(seg.ts.saturating_sub(t0)),
        speaker(seg),
        seg.text.trim()
    )
}

/// The transcript's pages, as ranges of segment indices, each within
/// [`PAGE_TOKENS`] (echo segments, the call leaking into the mic, are
/// skipped, as in summaries). Empty when nothing was said.
pub fn pages(segs: &[TranscriptSegment], t0: u64) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let (mut start, mut used) = (0, 0);
    for (i, seg) in segs.iter().enumerate() {
        if seg.echo {
            continue;
        }
        let tokens = estimate_tokens(&line(seg, t0)) + 1;
        if used > 0 && used + tokens > PAGE_TOKENS {
            out.push(start..i);
            (start, used) = (i, 0);
        }
        used += tokens;
    }
    if used > 0 {
        out.push(start..segs.len());
    }
    out
}

/// The 1-based page holding segment `seg`.
fn page_of(pages: &[Range<usize>], seg: usize) -> usize {
    pages
        .iter()
        .position(|r| r.contains(&seg))
        .map_or(1, |p| p + 1)
}

fn spoken(n: &Note) -> impl Iterator<Item = &TranscriptSegment> {
    n.transcript.iter().filter(|s| !s.echo)
}

/// When the meeting started (its first words, else when the note was made).
fn start_of(n: &Note) -> u64 {
    spoken(n).next().map_or(n.created_ts, |s| s.ts)
}

/// A meeting for lists: when, how long, who, and what there is to read.
pub fn meeting_summary(n: &Note) -> Value {
    let start = start_of(n);
    let end = spoken(n).last().map_or(start, |s| s.ts);
    json!({
        "id": n.id,
        "title": n.title,
        "folder": n.folder,
        "participants": n.participants,
        "createdTs": n.created_ts,
        "updatedTs": n.updated_ts,
        "startTs": start,
        "endTs": end,
        "durationSecs": end.saturating_sub(start),
        "lines": spoken(n).count(),
        "pages": pages(&n.transcript, t0(n)).len(),
        "hasSummary": !n.enhanced_content.trim().is_empty(),
        "hasNotes": !n.content.trim().is_empty(),
        "digests": n.digests.len(),
    })
}

/// `GET /v1/meetings/list`: meetings, newest first.
pub fn list_meetings(notes: &[Note], folder: Option<&str>, limit: usize) -> Vec<Value> {
    let mut meetings: Vec<&Note> = notes
        .iter()
        .filter(|n| is_meeting(n))
        .filter(|n| folder.is_none_or(|f| n.folder.eq_ignore_ascii_case(f)))
        .collect();
    meetings.sort_by_key(|n| std::cmp::Reverse(start_of(n)));
    meetings
        .into_iter()
        .take(limit)
        .map(meeting_summary)
        .collect()
}

/// The words a search looks for: lowercased, edge punctuation trimmed, three
/// letters or more (the same rule as the notes search).
pub fn query_words(query: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for w in query.to_lowercase().split_whitespace() {
        let w = w.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
        if w.chars().count() > 2 && !words.contains(&w) {
            words.push(w);
        }
    }
    words
}

fn hits(text: &str, words: &[String]) -> usize {
    let text = text.to_lowercase();
    words.iter().map(|w| text.matches(w.as_str()).count()).sum()
}

/// `text` on one line, at most `max` characters.
fn excerpt(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let cut: String = flat.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// `GET /v1/meetings/search`: meetings whose title, attendees, typed notes,
/// AI summary or transcript mention the query's words, best first, each with
/// its best-matching transcript lines (time order, with their page).
pub fn search_meetings(notes: &[Note], query: &str, limit: usize) -> Vec<Value> {
    let words = query_words(query);
    if words.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<(usize, u64, Value)> = Vec::new();
    for n in notes.iter().filter(|n| is_meeting(n)) {
        let mut score = hits(&n.title, &words) * 3
            + hits(&n.participants.join(" "), &words) * 2
            + hits(&n.content, &words)
            + hits(&n.enhanced_content, &words);
        let mut matched: Vec<(usize, usize)> = Vec::new(); // (distinct words, segment)
        for (i, seg) in n.transcript.iter().enumerate().filter(|(_, s)| !s.echo) {
            let text = seg.text.to_lowercase();
            let distinct = words.iter().filter(|w| text.contains(w.as_str())).count();
            if distinct > 0 {
                score += hits(&seg.text, &words);
                matched.push((distinct, i));
            }
        }
        if score == 0 {
            continue;
        }
        matched.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        matched.truncate(MAX_MATCHES);
        matched.sort_by_key(|m| m.1);
        let zero = t0(n);
        let pages = pages(&n.transcript, zero);
        let lines: Vec<Value> = matched
            .iter()
            .map(|&(_, i)| {
                let seg = &n.transcript[i];
                json!({
                    "at": clock(seg.ts.saturating_sub(zero)),
                    "speaker": speaker(seg),
                    "text": excerpt(&seg.text, 280),
                    "page": page_of(&pages, i),
                })
            })
            .collect();
        let summary_line = [&n.enhanced_content, &n.content]
            .iter()
            .flat_map(|t| t.lines())
            .find(|l| hits(l, &words) > 0)
            .map(|l| excerpt(l, 200));
        let mut item = meeting_summary(n);
        item["score"] = json!(score);
        item["matches"] = json!(lines);
        item["summaryMatch"] = json!(summary_line);
        found.push((score, start_of(n), item));
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    found.into_iter().take(limit).map(|(_, _, v)| v).collect()
}

// ---- reaching the running Yap ----

/// Why a call to the running Yap failed.
#[derive(Debug, Clone, PartialEq)]
pub enum BridgeError {
    /// No discovery file, nothing listening, or no answer: Yap isn't open
    /// (or its Local API is switched off).
    NotRunning,
    /// Yap answered with an error.
    Api { status: u16, message: String },
}

impl BridgeError {
    /// What the AI app's model reads.
    fn explain(&self) -> String {
        match self {
            BridgeError::NotRunning => NOT_RUNNING.to_string(),
            BridgeError::Api { status: 401, .. } => "Yap didn't accept the connection (its local API file is out of date). Ask the user to restart Yap, then try again.".to_string(),
            BridgeError::Api { message, .. } => message.clone(),
        }
    }
}

/// A route of the running Yap's local API. Blocking; the server calls it off
/// the async threads.
pub trait Bridge: Send + Sync + 'static {
    /// `method` + `path` (with its query) → the response's JSON body.
    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, BridgeError>;
}

/// The real bridge: loopback HTTP to the port in the discovery file, with
/// its bearer token. Both are re-read on every call, so a restarted Yap
/// (new port, new token) is found again without restarting the AI app.
pub struct HttpBridge {
    file: PathBuf,
    timeout: Duration,
}

impl HttpBridge {
    pub fn new(file: PathBuf) -> Self {
        Self {
            file,
            timeout: Duration::from_secs(15),
        }
    }

    fn endpoint(&self) -> Option<(u16, String)> {
        let raw = std::fs::read_to_string(&self.file).ok()?;
        let v: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}')).ok()?;
        let port = u16::try_from(v.get("port")?.as_u64()?).ok()?;
        let token = v.get("token")?.as_str()?.to_string();
        Some((port, token))
    }
}

/// The largest response read from the bridge (a long meeting is ~200 KB).
const MAX_RESPONSE: u64 = 32 * 1024 * 1024;

impl Bridge for HttpBridge {
    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, BridgeError> {
        let (port, token) = self.endpoint().ok_or(BridgeError::NotRunning)?;
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let mut stream = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2))
            .map_err(|_| BridgeError::NotRunning)?;
        let _ = stream.set_read_timeout(Some(self.timeout));
        let _ = stream.set_write_timeout(Some(self.timeout));
        let payload = body.map(Value::to_string).unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|_| BridgeError::NotRunning)?;
        let mut raw = Vec::new();
        if stream.take(MAX_RESPONSE).read_to_end(&mut raw).is_err() && raw.is_empty() {
            return Err(BridgeError::NotRunning);
        }
        let (status, body) = parse_http_response(&raw).ok_or(BridgeError::NotRunning)?;
        let json: Value = if body.iter().all(u8::is_ascii_whitespace) {
            Value::Null
        } else {
            serde_json::from_slice(&body).map_err(|_| BridgeError::Api {
                status,
                message: "Yap sent a reply that couldn't be read.".to_string(),
            })?
        };
        if (200..300).contains(&status) {
            return Ok(json);
        }
        let message = json["error"]["message"]
            .as_str()
            .unwrap_or("Yap couldn't do that.")
            .to_string();
        Err(BridgeError::Api { status, message })
    }
}

/// `(status, body)` from a raw HTTP/1.x response (chunked or not).
fn parse_http_response(raw: &[u8]) -> Option<(u16, Vec<u8>)> {
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&raw[..split]).ok()?;
    let rest = &raw[split + 4..];
    let mut lines = head.split("\r\n");
    let status: u16 = lines.next()?.split_whitespace().nth(1)?.parse().ok()?;
    let chunked = lines.any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    if !chunked {
        return Some((status, rest.to_vec()));
    }
    let (mut body, mut at) = (Vec::new(), 0);
    loop {
        let eol = at + rest.get(at..)?.windows(2).position(|w| w == b"\r\n")?;
        let size_text = std::str::from_utf8(&rest[at..eol]).ok()?;
        let size = usize::from_str_radix(size_text.split(';').next()?.trim(), 16).ok()?;
        if size == 0 {
            return Some((status, body));
        }
        body.extend_from_slice(rest.get(eol + 2..eol + 2 + size)?);
        at = eol + 2 + size + 2;
    }
}

// ---- the server ----

/// Yap's MCP server over some [`Bridge`] (the real one, or a test fake).
pub struct YapMcp<B: Bridge> {
    bridge: Arc<B>,
}

impl<B: Bridge> YapMcp<B> {
    pub fn new(bridge: B) -> Self {
        Self {
            bridge: Arc::new(bridge),
        }
    }

    async fn call(
        &self,
        method: &'static str,
        path: String,
        body: Option<Value>,
    ) -> Result<Value, BridgeError> {
        let bridge = Arc::clone(&self.bridge);
        tokio::task::spawn_blocking(move || bridge.call(method, &path, body.as_ref()))
            .await
            .unwrap_or(Err(BridgeError::NotRunning))
    }

    async fn get(&self, path: String) -> Result<Value, BridgeError> {
        self.call("GET", path, None).await
    }

    /// Whether the person lets AI apps save notes (asked of Yap each time;
    /// unreachable = no).
    async fn writes_allowed(&self) -> bool {
        self.get("/v1/mcp/config".to_string())
            .await
            .is_ok_and(|v| v["data"]["allowWrites"] == json!(true))
    }

    async fn run_tool(&self, name: &str, args: &JsonObject) -> Result<String, String> {
        let fail = |e: BridgeError| e.explain();
        match name {
            "list_meetings" => {
                let limit = arg_u64(args, "limit").unwrap_or(20).clamp(1, 100);
                let v = self
                    .get(format!("/v1/meetings/list?limit={limit}"))
                    .await
                    .map_err(fail)?;
                Ok(render_meeting_list(&v["data"]))
            }
            "search_meetings" => {
                let query = arg_str(args, "query").unwrap_or("");
                if query_words(query).is_empty() {
                    return Err("Search with at least one word of three or more letters (a name, a topic, something that was said).".to_string());
                }
                let limit = arg_u64(args, "limit").unwrap_or(5).clamp(1, 20);
                let v = self
                    .get(format!(
                        "/v1/meetings/search?q={}&limit={limit}",
                        encode(query)
                    ))
                    .await
                    .map_err(fail)?;
                Ok(render_meeting_search(query, &v["data"]))
            }
            "get_meeting" => {
                let id = arg_u64(args, "id").ok_or("Give the meeting's id (from list_meetings or search_meetings).")?;
                let page = arg_u64(args, "transcript_page").unwrap_or(1).max(1) as usize;
                let note = self.note(id).await?;
                if !is_meeting(&note) {
                    return Err(format!(
                        "Note {id} isn't a meeting (it has no recording); read it with get_note."
                    ));
                }
                render_meeting(&note, page)
            }
            "search_notes" => {
                let query = arg_str(args, "query").unwrap_or("");
                if query_words(query).is_empty() {
                    return Err("Search with at least one word of three or more letters.".to_string());
                }
                let limit = arg_u64(args, "limit").unwrap_or(10).clamp(1, 30);
                let v = self
                    .get(format!("/v1/notes/search?q={}&limit={limit}", encode(query)))
                    .await
                    .map_err(fail)?;
                Ok(render_note_search(query, &v["data"]))
            }
            "get_note" => {
                let id = arg_u64(args, "id").ok_or("Give the note's id (from search_notes or list_meetings).")?;
                Ok(render_note(&self.note(id).await?))
            }
            "list_folders" => {
                let folders = self.get("/v1/folders/list".to_string()).await.map_err(fail)?;
                let notes = self
                    .get("/v1/notes/list?limit=1000000".to_string())
                    .await
                    .map_err(fail)?;
                Ok(render_folders(&folders["data"], &notes["data"]))
            }
            "create_note" => {
                if !self.writes_allowed().await {
                    return Err("Saving notes from AI apps is switched off in Yap. The user can switch it on in Yap: Settings → MCP → \"Let AI apps save notes to Yap\".".to_string());
                }
                let title = arg_str(args, "title").unwrap_or("").trim().to_string();
                let content = arg_str(args, "content").unwrap_or("").to_string();
                if title.is_empty() && content.trim().is_empty() {
                    return Err("A note needs a title or some content.".to_string());
                }
                let folder = arg_str(args, "folder").unwrap_or("").trim().to_string();
                if !folder.is_empty() {
                    self.call(
                        "POST",
                        "/v1/folders/create".to_string(),
                        Some(json!({ "name": folder })),
                    )
                    .await
                    .map_err(fail)?;
                }
                let v = self
                    .call(
                        "POST",
                        "/v1/notes/create".to_string(),
                        Some(json!({
                            "title": title,
                            "content": content,
                            "folder": folder,
                            "source": "mcp",
                        })),
                    )
                    .await
                    .map_err(fail)?;
                let note = &v["data"];
                Ok(format!(
                    "Saved “{}” to Yap (note id {}, folder {}).",
                    note["title"].as_str().unwrap_or(&title),
                    note["id"],
                    note["folder"].as_str().unwrap_or("Personal")
                ))
            }
            other => Err(format!("Unknown tool: {other}")),
        }
    }

    async fn note(&self, id: u64) -> Result<Note, String> {
        match self.get(format!("/v1/notes/{id}")).await {
            Ok(v) => serde_json::from_value(v["data"].clone())
                .map_err(|_| "Yap sent a note that couldn't be read.".to_string()),
            Err(BridgeError::Api { status: 404, .. }) => Err(format!(
                "There's no note with id {id}. Find ids with list_meetings, search_meetings or search_notes."
            )),
            Err(e) => Err(e.explain()),
        }
    }
}

fn arg_str<'a>(args: &'a JsonObject, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

/// A whole number, also when a model sends it as a string ("12") or a float
/// ("12.0").
fn arg_u64(args: &JsonObject, key: &str) -> Option<u64> {
    match args.get(key)? {
        Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_f64().filter(|f| *f >= 0.0 && f.fract() == 0.0).map(|f| f as u64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Percent-encode a query value.
fn encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

fn schema(v: Value) -> Arc<JsonObject> {
    Arc::new(v.as_object().cloned().unwrap_or_default())
}

fn read_only(title: &str) -> ToolAnnotations {
    ToolAnnotations::with_title(title)
        .read_only(true)
        .open_world(false)
}

/// The tools an AI app sees; `create_note` only when writes are allowed.
pub fn tools(writes: bool) -> Vec<Tool> {
    let mut tools = vec![
        Tool::new(
            "list_meetings",
            "List the user's recent meetings recorded with Yap, newest first: id, title, date and time, length, attendees, and whether an AI summary exists. Use it to find a meeting by date (\"yesterday's standup\"), then read it with get_meeting.",
            schema(json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "How many meetings to list (default 20)." }
                },
                "additionalProperties": false
            })),
        )
        .annotate(read_only("List meetings")),
        Tool::new(
            "search_meetings",
            "Search the user's Yap meetings by keywords across titles, attendees, notes typed during the meeting, AI summaries and transcripts. Returns matching meetings, best first, with their best-matching transcript lines (timestamped, with the transcript page each is on). Use specific words: names, topics, numbers. Words under three letters are ignored.",
            schema(json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Words to look for, e.g. \"budget Priya\"." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 20, "description": "How many meetings to return (default 5)." }
                },
                "required": ["query"],
                "additionalProperties": false
            })),
        )
        .annotate(read_only("Search meetings")),
        Tool::new(
            "get_meeting",
            "Read one meeting by id: when it was, who attended, the AI summary or action plan, notes the user typed during it, and the transcript as timestamped lines. 'You' is the user; 'Them' is everyone else on the call (one voice: speakers aren't told apart). Long transcripts come in pages of about 20 minutes: the reply says how many there are, and transcript_page fetches the next.",
            schema(json!({
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "The meeting's id." },
                    "transcript_page": { "type": "integer", "minimum": 1, "description": "Which page of the transcript (default 1). Pages after the first leave out the summary." }
                },
                "required": ["id"],
                "additionalProperties": false
            })),
        )
        .annotate(read_only("Read a meeting")),
        Tool::new(
            "search_notes",
            "Search all of the user's Yap notes (meeting notes and their own notes) by keywords in titles and text. Returns ids, titles, folders, dates and a preview. Read one with get_note, or get_meeting for a meeting's transcript.",
            schema(json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Words to look for." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 30, "description": "How many notes to return (default 10)." }
                },
                "required": ["query"],
                "additionalProperties": false
            })),
        )
        .annotate(read_only("Search notes")),
        Tool::new(
            "get_note",
            "Read one Yap note by id: its text and, when there is one, its AI-enhanced version. For a meeting note this gives the summary and the notes typed during it; use get_meeting for the transcript.",
            schema(json!({
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "The note's id." }
                },
                "required": ["id"],
                "additionalProperties": false
            })),
        )
        .annotate(read_only("Read a note")),
        Tool::new(
            "list_folders",
            "List the user's Yap note folders and how many notes each holds.",
            schema(json!({ "type": "object", "properties": {}, "additionalProperties": false })),
        )
        .annotate(read_only("List folders")),
    ];
    if writes {
        tools.push(
            Tool::new(
                "create_note",
                "Save a new note to the user's Yap notes, for example when they ask to save a summary, a draft or a list to Yap. Markdown is fine. It goes into the given folder (created if it doesn't exist), else Personal.",
                schema(json!({
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "description": "The note's title." },
                        "content": { "type": "string", "description": "The note's text (markdown)." },
                        "folder": { "type": "string", "description": "Folder name (optional)." }
                    },
                    "required": ["title", "content"],
                    "additionalProperties": false
                })),
            )
            .annotate(
                ToolAnnotations::with_title("Save a note to Yap")
                    .read_only(false)
                    .destructive(false)
                    .idempotent(false)
                    .open_world(false),
            ),
        );
    }
    tools
}

impl<B: Bridge> ServerHandler for YapMcp<B> {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("yap", env!("CARGO_PKG_VERSION"))
                    .with_title("Yap")
                    .with_description("Your Yap meeting notes and notes, read on this PC")
                    .with_website_url("https://github.com/nayballs/Yap"),
            )
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let result = ListToolsResult::with_all_items(tools(self.writes_allowed().await));
        // 2026-07-28 wants a freshness hint on lists; older clients don't
        // know the fields. Short, as create_note follows a setting.
        let modern = context
            .protocol_version()
            .is_some_and(|v| v.as_str() >= ProtocolVersion::V_2026_07_28.as_str());
        Ok(if modern {
            result
                .with_ttl_ms(60_000)
                .with_cache_scope(CacheScope::Private)
        } else {
            result
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let known = tools(true).iter().any(|t| t.name == request.name);
        if !known {
            return Err(McpError::invalid_params(
                format!("Unknown tool: {}", request.name),
                None,
            ));
        }
        let args = request.arguments.unwrap_or_default();
        let result = match self.run_tool(&request.name, &args).await {
            Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
            Err(message) => CallToolResult::error(vec![ContentBlock::text(message)]),
        };
        Ok(result.into())
    }
}

// ---- what the model reads ----

fn local(ts: u64) -> chrono::DateTime<chrono::Local> {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(i64::try_from(ts).unwrap_or(0), 0)
        .single()
        .unwrap_or_else(|| chrono::Local.timestamp_opt(0, 0).unwrap())
}

/// "Mon 5 Oct 2026, 14:30".
fn when(ts: u64) -> String {
    local(ts).format("%a %-d %b %Y, %H:%M").to_string()
}

/// "42 min", "1 h 05 min", "under a minute".
fn length(secs: u64) -> String {
    let mins = (secs + 30) / 60;
    match mins {
        0 => "under a minute".to_string(),
        m if m < 60 => format!("{m} min"),
        m => format!("{} h {:02} min", m / 60, m % 60),
    }
}

fn title_of(title: &str, fallback: &str) -> String {
    let t = title.trim();
    if t.is_empty() {
        fallback.to_string()
    } else {
        t.to_string()
    }
}

fn names(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// One meeting as a list line (from a bridge summary).
fn meeting_line(m: &Value) -> String {
    let mut parts = vec![
        format!("id {}", m["id"]),
        format!("**{}**", title_of(m["title"].as_str().unwrap_or(""), "Untitled meeting")),
        when(m["startTs"].as_u64().unwrap_or(0)),
    ];
    if m["lines"].as_u64().unwrap_or(0) > 0 {
        parts.push(length(m["durationSecs"].as_u64().unwrap_or(0)));
    } else {
        parts.push("no transcript".to_string());
    }
    let people = names(&m["participants"]);
    if !people.is_empty() {
        parts.push(format!("with {}", people.join(", ")));
    }
    if m["hasSummary"] == json!(true) {
        parts.push("AI summary".to_string());
    }
    let pages = m["pages"].as_u64().unwrap_or(0);
    if pages > 1 {
        parts.push(format!("transcript in {pages} pages"));
    }
    format!("- {}", parts.join(" · "))
}

fn render_meeting_list(data: &Value) -> String {
    let items = data.as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return "No meetings recorded in Yap yet.".to_string();
    }
    let mut out = format!(
        "{} meeting{} in Yap, newest first:\n",
        items.len(),
        if items.len() == 1 { "" } else { "s" }
    );
    for m in &items {
        out.push_str(&meeting_line(m));
        out.push('\n');
    }
    out.push_str("\nRead one with get_meeting and its id.");
    out
}

fn render_meeting_search(query: &str, data: &Value) -> String {
    let items = data.as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return format!("No meetings mention “{}”.", query.trim());
    }
    let mut out = format!(
        "{} meeting{} mention “{}”, best first:\n",
        items.len(),
        if items.len() == 1 { "" } else { "s" },
        query.trim()
    );
    for m in &items {
        out.push_str(&meeting_line(m));
        out.push('\n');
        if let Some(s) = m["summaryMatch"].as_str() {
            out.push_str(&format!("  - in the summary or notes: {s}\n"));
        }
        for l in m["matches"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "  - [{}] {}: {} (transcript page {})\n",
                l["at"].as_str().unwrap_or(""),
                l["speaker"].as_str().unwrap_or(""),
                l["text"].as_str().unwrap_or(""),
                l["page"]
            ));
        }
    }
    out.push_str("\nRead a meeting with get_meeting (id, and transcript_page for a later part).");
    out
}

/// A meeting for the model: what it was, the summary, typed notes, and one
/// page of transcript (page 1 also carries the summary; later pages only the
/// transcript, to spare the context).
pub fn render_meeting(n: &Note, page: usize) -> Result<String, String> {
    let zero = t0(n);
    let pages = pages(&n.transcript, zero);
    let total = pages.len();
    if total > 0 && page > total {
        return Err(format!(
            "Meeting {} has {total} transcript page{}: ask for 1 to {total}.",
            n.id,
            if total == 1 { "" } else { "s" }
        ));
    }
    let start = start_of(n);
    let end = spoken(n).last().map_or(start, |s| s.ts);
    let mut out = format!("# {}\n", title_of(&n.title, "Untitled meeting"));
    let span = if total > 0 {
        format!(
            "{}–{} ({})",
            when(start),
            local(end).format("%H:%M"),
            length(end.saturating_sub(start))
        )
    } else {
        format!("{} (no transcript)", when(start))
    };
    out.push_str(&format!(
        "Meeting · {span} · {} · id {}\n",
        local(start).format("UTC%:z"),
        n.id
    ));
    let people: Vec<&str> = n
        .participants
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    if !people.is_empty() {
        out.push_str(&format!("Attendees: {}\n", people.join(", ")));
    }
    out.push_str(&format!("Folder: {}\n", n.folder));

    if page <= 1 {
        let summary = n.enhanced_content.trim();
        if !summary.is_empty() {
            out.push_str(&format!("\n## AI summary\n{summary}\n"));
        }
        let typed = n.content.trim();
        if !typed.is_empty() {
            out.push_str(&format!("\n## Notes typed during the meeting\n{typed}\n"));
        }
        if summary.is_empty() && !n.digests.is_empty() {
            out.push_str(&format!(
                "\n## AI notes written while it was recorded\n{}",
                render_digests(&n.digests, zero, DIGEST_TOKENS)
            ));
        }
    }

    let Some(range) = pages.get(page.saturating_sub(1)) else {
        out.push_str("\n## Transcript\nNothing was transcribed for this meeting.\n");
        return Ok(out);
    };
    let lines: Vec<String> = n.transcript[range.clone()]
        .iter()
        .filter(|s| !s.echo)
        .map(|s| line(s, zero))
        .collect();
    if total > 1 {
        let first = n.transcript[range.clone()].iter().find(|s| !s.echo);
        let last = n.transcript[range.clone()].iter().rev().find(|s| !s.echo);
        let at = |s: Option<&TranscriptSegment>| clock(s.map_or(0, |s| s.ts.saturating_sub(zero)));
        out.push_str(&format!(
            "\n## Transcript, page {page} of {total} ({}–{})\n",
            at(first),
            at(last)
        ));
    } else {
        out.push_str("\n## Transcript\n");
    }
    out.push_str("'You' is the person using Yap; 'Them' is everyone else on the call (not told apart).\n");
    out.push_str(&lines.join("\n"));
    out.push('\n');
    if page < total {
        out.push_str(&format!(
            "\n(More: call get_meeting with id {} and transcript_page {}.)\n",
            n.id,
            page + 1
        ));
    }
    Ok(out)
}

/// A note for the model: its AI-enhanced version, its text, and for a
/// meeting a pointer to the transcript.
pub fn render_note(n: &Note) -> String {
    let meeting = is_meeting(n);
    let mut out = format!(
        "# {}\n",
        title_of(&n.title, if meeting { "Untitled meeting" } else { "Untitled note" })
    );
    out.push_str(&format!(
        "{} · folder {} · created {} · updated {} · id {}\n",
        if meeting { "Meeting note" } else { "Note" },
        n.folder,
        when(n.created_ts),
        when(n.updated_ts),
        n.id
    ));
    let people: Vec<&str> = n
        .participants
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    if !people.is_empty() {
        out.push_str(&format!("Attendees: {}\n", people.join(", ")));
    }
    let enhanced = n.enhanced_content.trim();
    if !enhanced.is_empty() {
        out.push_str(&format!(
            "\n## {}\n{enhanced}\n",
            if meeting { "AI summary" } else { "AI-enhanced version" }
        ));
    }
    let text = n.content.trim();
    out.push_str(&format!(
        "\n## {}\n{}\n",
        if meeting { "Notes typed during the meeting" } else { "Text" },
        if text.is_empty() { "(empty)" } else { text }
    ));
    let said = spoken(n).count();
    if meeting && said > 0 {
        out.push_str(&format!(
            "\nThis meeting has a transcript of {said} line{}: read it with get_meeting (id {}).\n",
            if said == 1 { "" } else { "s" },
            n.id
        ));
    }
    out
}

fn render_note_search(query: &str, data: &Value) -> String {
    let items = data.as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return format!("No notes mention “{}”.", query.trim());
    }
    let mut out = format!(
        "{} note{} mention “{}”, best first:\n",
        items.len(),
        if items.len() == 1 { "" } else { "s" },
        query.trim()
    );
    for n in &items {
        let meeting = n["noteType"].as_str() == Some("meeting");
        let ts = n["createdTs"]
            .as_u64()
            .or_else(|| n["updatedTs"].as_u64())
            .unwrap_or(0);
        out.push_str(&format!(
            "- id {} · **{}** · {} · {} · {}\n",
            n["id"],
            title_of(n["title"].as_str().unwrap_or(""), "Untitled"),
            if meeting { "meeting" } else { "note" },
            n["folder"].as_str().unwrap_or(""),
            when(ts)
        ));
        let preview = excerpt(n["preview"].as_str().unwrap_or(""), 200);
        if !preview.is_empty() {
            out.push_str(&format!("  {preview}\n"));
        }
    }
    out.push_str("\nRead one with get_note (or get_meeting for a meeting's transcript).");
    out
}

fn render_folders(folders: &Value, notes: &Value) -> String {
    let names: Vec<&str> = folders
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if names.is_empty() {
        return "No folders yet.".to_string();
    }
    let notes = notes.as_array().cloned().unwrap_or_default();
    let mut out = String::from("Folders in Yap:\n");
    for name in names {
        let count = notes
            .iter()
            .filter(|n| {
                n["folder"]
                    .as_str()
                    .is_some_and(|f| f.eq_ignore_ascii_case(name))
            })
            .count();
        out.push_str(&format!(
            "- {name}: {count} note{}\n",
            if count == 1 { "" } else { "s" }
        ));
    }
    out
}

// ---- `yap.exe mcp` ----

/// Serve MCP on stdin/stdout until the AI app hangs up. Returns the exit
/// code. Logs go to stderr, which clients keep (stdout carries only
/// protocol messages).
pub fn run_stdio() -> i32 {
    let filter = tracing_subscriber::EnvFilter::try_from_env("YAP_MCP_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,yap_lib=info"));
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(filter)
        .try_init();
    let file = crate::bridge::bridge_file_path();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "yap mcp: serving over stdio; Yap's local API is found through {}",
        file.display()
    );
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!("yap mcp: no async runtime: {e}");
            return 1;
        }
    };
    runtime.block_on(async move {
        let server = YapMcp::new(HttpBridge::new(file));
        match server.serve(rmcp::transport::stdio()).await {
            Ok(running) => match running.waiting().await {
                Ok(_) => 0,
                Err(e) => {
                    tracing::error!("yap mcp: stopped: {e}");
                    1
                }
            },
            // The client left before saying hello (a probe, or it quit).
            Err(rmcp::service::ServerInitializeError::ConnectionClosed(_)) => 0,
            Err(e) => {
                tracing::error!("yap mcp: couldn't start a session: {e}");
                1
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::MeetingDigest;
    use std::sync::Mutex;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    fn seg(source: &str, text: &str, ts: u64) -> TranscriptSegment {
        TranscriptSegment {
            source: source.to_string(),
            text: text.to_string(),
            ts,
            echo: false,
            dictated: false,
        }
    }

    fn note(id: u64, title: &str, kind: &str) -> Note {
        serde_json::from_value(json!({
            "id": id,
            "title": title,
            "noteType": kind,
            "folder": if kind == "meeting" { "Meetings" } else { "Personal" },
            "createdTs": 1_759_000_000u64 + id,
            "updatedTs": 1_759_000_000u64 + id,
        }))
        .unwrap()
    }

    /// A meeting with a short transcript and an action plan.
    fn standup() -> Note {
        let mut n = note(1, "Q3 planning sync", "meeting");
        n.participants = vec!["Priya".into(), "Tom".into()];
        n.enhanced_content = "## Action plan\n### Priya\n- [ ] send the revised budget (due: Friday)".into();
        n.content = "remember the venue".into();
        let t = 1_759_000_100;
        n.transcript = vec![
            seg("you", "Let's start with the budget.", t),
            seg("them", "Priya here: I will send the revised budget by Friday.", t + 15),
            seg("you", "Great, and Tom books the venue.", t + 30),
        ];
        n
    }

    /// A meeting long enough for several transcript pages.
    fn long_meeting() -> Note {
        let mut n = note(2, "Offsite planning", "meeting");
        let t = 1_759_100_000;
        n.transcript = (0..900)
            .map(|i| {
                seg(
                    if i % 2 == 0 { "you" } else { "them" },
                    &format!("LINE{i:04} we walked through the plan for the offsite and how the two days should flow, covering travel and rooms"),
                    t + i * 15,
                )
            })
            .collect();
        n.transcript[450].text = "LINE0450 the catering budget is the open question".into();
        n.digests = vec![MeetingDigest {
            from_seg: 0,
            to_seg: 40,
            start_ts: t,
            end_ts: t + 600,
            key_points: vec!["The offsite plan was walked through.".into()],
            decisions: vec![],
            actions: vec![],
            questions: vec!["Who handles catering?".into()],
            raw: String::new(),
        }];
        n
    }

    #[test]
    fn pages_cover_every_line_once_within_budget() {
        let n = long_meeting();
        let zero = t0(&n);
        let p = pages(&n.transcript, zero);
        assert!(p.len() >= 3, "{} pages", p.len());
        assert_eq!(p.first().unwrap().start, 0);
        assert_eq!(p.last().unwrap().end, n.transcript.len());
        for w in p.windows(2) {
            assert_eq!(w[0].end, w[1].start);
        }
        for r in &p {
            let text: String = n.transcript[r.clone()].iter().map(|s| line(s, zero) + "\n").collect();
            assert!(estimate_tokens(&text) <= PAGE_TOKENS + 10);
        }
        assert_eq!(page_of(&p, 0), 1);
        assert_eq!(page_of(&p, n.transcript.len() - 1), p.len());
        // Echo lines don't count, and a transcript of only echo has no pages.
        let mut echo = standup();
        echo.transcript.iter_mut().for_each(|s| s.echo = true);
        assert!(pages(&echo.transcript, 0).is_empty());
    }

    #[test]
    fn meetings_are_listed_newest_first_and_searched_with_their_lines() {
        let notes = vec![standup(), long_meeting(), note(3, "Groceries budget", "personal")];
        let list = list_meetings(&notes, None, 10);
        assert_eq!(list.len(), 2, "the personal note isn't a meeting");
        assert_eq!(list[0]["id"], 2);
        assert_eq!(list[1]["participants"], json!(["Priya", "Tom"]));
        assert_eq!(list[1]["durationSecs"], 30);
        assert_eq!(list[1]["hasSummary"], true);
        assert_eq!(list_meetings(&notes, Some("personal"), 10).len(), 0);

        let found = search_meetings(&notes, "budget", 5);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0]["id"], 1, "the most mentions first");
        let lines = found[0]["matches"].as_array().unwrap();
        assert_eq!(lines[0]["at"], "0:00");
        assert_eq!(lines[1]["speaker"], "Them");
        assert!(found[0]["summaryMatch"].as_str().unwrap().contains("revised budget"));
        let catering = &search_meetings(&notes, "catering", 5)[0];
        assert_eq!(catering["id"], 2);
        let page = catering["matches"][0]["page"].as_u64().unwrap();
        assert!(page > 1, "line 450 is past the first page");
        // An attendee's name finds their meetings; short words find nothing.
        assert_eq!(search_meetings(&notes, "tom", 5)[0]["id"], 1);
        assert!(search_meetings(&notes, "is a", 5).is_empty());
    }

    #[test]
    fn a_meeting_reads_as_summary_then_pages() {
        let n = standup();
        let text = render_meeting(&n, 1).unwrap();
        assert!(text.starts_with("# Q3 planning sync\n"));
        assert!(text.contains("Attendees: Priya, Tom"));
        assert!(text.contains("## AI summary\n## Action plan"));
        assert!(text.contains("## Notes typed during the meeting\nremember the venue"));
        assert!(text.contains("[0:15] Them: Priya here: I will send the revised budget by Friday."));
        assert!(!text.contains("transcript_page"), "one page, no paging hint");

        let long = long_meeting();
        let first = render_meeting(&long, 1).unwrap();
        assert!(first.contains("## AI notes written while it was recorded"));
        assert!(first.contains("Who handles catering?"));
        assert!(first.contains("## Transcript, page 1 of"));
        assert!(first.contains("transcript_page 2"));
        let second = render_meeting(&long, 2).unwrap();
        assert!(!second.contains("AI notes"), "later pages are transcript only");
        assert!(!second.contains("LINE0000"));
        assert!(render_meeting(&long, 99).unwrap_err().contains("transcript pages"));
        assert!(estimate_tokens(&second) <= PAGE_TOKENS + 400);
    }

    #[test]
    fn http_responses_parse_plain_and_chunked() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"data\":[]}";
        assert_eq!(parse_http_response(plain), Some((200, b"{\"data\":[]}".to_vec())));
        let chunked = b"HTTP/1.1 404 Not Found\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n3\r\n:1}\r\n0\r\n\r\n";
        assert_eq!(parse_http_response(chunked), Some((404, b"{\"a\":1}".to_vec())));
        assert_eq!(parse_http_response(b"garbage"), None);
    }

    #[test]
    fn the_http_bridge_reads_the_discovery_file_and_sends_the_token() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let dir = std::env::temp_dir().join(format!("yap-mcp-bridge-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("cli-bridge.json");
        std::fs::write(&file, json!({ "version": 1, "port": port, "token": "t0k" }).to_string()).unwrap();
        let handle = std::thread::spawn(move || {
            for _ in 0..2 {
                let req = server.recv().unwrap();
                let authed = req
                    .headers()
                    .iter()
                    .any(|h| h.field.equiv("Authorization") && h.value.as_str() == "Bearer t0k");
                let (status, body) = match (authed, req.url()) {
                    (false, _) => (401, json!({ "error": { "code": "unauthorized", "message": "Unauthorized" } })),
                    (true, "/v1/notes/9") => (404, json!({ "error": { "code": "not_found", "message": "Note 9 not found" } })),
                    (true, _) => (200, json!({ "data": { "ok": true } })),
                };
                let _ = req.respond(tiny_http::Response::from_string(body.to_string()).with_status_code(status));
            }
        });
        let bridge = HttpBridge::new(file.clone());
        assert_eq!(bridge.call("GET", "/v1/health", None).unwrap()["data"]["ok"], true);
        assert_eq!(
            bridge.call("GET", "/v1/notes/9", None),
            Err(BridgeError::Api { status: 404, message: "Note 9 not found".into() })
        );
        handle.join().unwrap();
        // No file (Yap closed, or the Local API off) and a dead port both
        // read as "not running".
        assert_eq!(HttpBridge::new(dir.join("missing.json")).call("GET", "/v1/health", None), Err(BridgeError::NotRunning));
        assert_eq!(bridge.call("GET", "/v1/health", None), Err(BridgeError::NotRunning));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The running Yap, faked: the meeting routes run the real views above.
    struct FakeBridge {
        notes: Vec<Note>,
        writes: bool,
        up: bool,
        created: Mutex<Vec<Value>>,
    }

    impl Bridge for FakeBridge {
        fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, BridgeError> {
            if !self.up {
                return Err(BridgeError::NotRunning);
            }
            let (route, query) = path.split_once('?').unwrap_or((path, ""));
            let param = |k: &str| {
                url::form_urlencoded::parse(query.as_bytes())
                    .find(|(key, _)| key == k)
                    .map(|(_, v)| v.into_owned())
            };
            let limit = param("limit").and_then(|l| l.parse().ok()).unwrap_or(50);
            match (method, route) {
                ("GET", "/v1/mcp/config") => Ok(json!({ "data": { "allowWrites": self.writes } })),
                ("GET", "/v1/meetings/list") => Ok(json!({ "data": list_meetings(&self.notes, None, limit) })),
                ("GET", "/v1/meetings/search") => Ok(json!({ "data": search_meetings(&self.notes, &param("q").unwrap_or_default(), limit) })),
                ("GET", "/v1/folders/list") => Ok(json!({ "data": ["Personal", "Meetings"] })),
                ("GET", "/v1/notes/list") => Ok(json!({ "data": self.notes.iter().map(|n| json!({ "id": n.id, "folder": n.folder })).collect::<Vec<_>>() })),
                ("POST", "/v1/folders/create") => Ok(json!({ "data": {} })),
                ("POST", "/v1/notes/create") => {
                    self.created.lock().unwrap().push(body.cloned().unwrap_or_default());
                    Ok(json!({ "data": { "id": 77, "title": body.unwrap()["title"], "folder": "Ideas" } }))
                }
                ("GET", r) if r.starts_with("/v1/notes/") => {
                    let id: u64 = r.trim_start_matches("/v1/notes/").parse().unwrap_or(0);
                    match self.notes.iter().find(|n| n.id == id) {
                        Some(n) => Ok(json!({ "data": n })),
                        None => Err(BridgeError::Api { status: 404, message: format!("Note {id} not found") }),
                    }
                }
                _ => Err(BridgeError::Api { status: 404, message: "Not found".into() }),
            }
        }
    }

    fn fake(up: bool, writes: bool) -> FakeBridge {
        FakeBridge {
            notes: vec![standup(), long_meeting(), note(3, "Groceries", "personal")],
            writes,
            up,
            created: Mutex::new(Vec::new()),
        }
    }

    /// A JSON-RPC client on one end of an in-memory pipe, the server on the
    /// other: exactly what an AI app sends over stdio.
    struct Session {
        tx: tokio::io::WriteHalf<tokio::io::DuplexStream>,
        rx: tokio::io::Lines<BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
        next_id: u64,
    }

    impl Session {
        fn start(bridge: FakeBridge) -> Self {
            let (client, server) = tokio::io::duplex(1 << 20);
            let (sr, sw) = tokio::io::split(server);
            tokio::spawn(async move {
                if let Ok(running) = YapMcp::new(bridge).serve((sr, sw)).await {
                    let _ = running.waiting().await;
                }
            });
            let (cr, cw) = tokio::io::split(client);
            Session { tx: cw, rx: BufReader::new(cr).lines(), next_id: 1 }
        }

        async fn send(&mut self, msg: Value) {
            self.tx.write_all(format!("{msg}\n").as_bytes()).await.unwrap();
        }

        async fn request(&mut self, method: &str, params: Value) -> Value {
            let id = self.next_id;
            self.next_id += 1;
            self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).await;
            loop {
                let line = tokio::time::timeout(Duration::from_secs(10), self.rx.next_line())
                    .await
                    .expect("a reply in time")
                    .unwrap()
                    .expect("the server is still there");
                let v: Value = serde_json::from_str(&line).unwrap();
                if v["id"] == json!(id) {
                    return v;
                }
            }
        }

        async fn initialize(&mut self, version: &str) -> Value {
            let r = self
                .request(
                    "initialize",
                    json!({
                        "protocolVersion": version,
                        "capabilities": {},
                        "clientInfo": { "name": "test-client", "version": "1.0" }
                    }),
                )
                .await;
            self.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await;
            r
        }

        async fn tool(&mut self, name: &str, args: Value) -> (bool, String) {
            let r = self.request("tools/call", json!({ "name": name, "arguments": args })).await;
            let result = &r["result"];
            (
                result["isError"] == json!(true),
                result["content"][0]["text"].as_str().unwrap_or_default().to_string(),
            )
        }
    }

    #[tokio::test]
    async fn initialize_list_and_call_like_a_2025_client() {
        let mut s = Session::start(fake(true, false));
        let init = s.initialize("2025-06-18").await;
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(init["result"]["serverInfo"]["name"], "yap");
        assert!(init["result"]["capabilities"]["tools"].is_object());
        assert!(init["result"]["instructions"].as_str().unwrap().contains("never available"));

        let list = s.request("tools/list", json!({})).await;
        let names: Vec<&str> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["list_meetings", "search_meetings", "get_meeting", "search_notes", "get_note", "list_folders"]);
        assert!(list["result"].get("resultType").is_none(), "legacy peers get the legacy shape");
        assert!(list["result"]["tools"][0]["annotations"]["readOnlyHint"] == json!(true));

        let (err, text) = s.tool("list_meetings", json!({})).await;
        assert!(!err);
        assert!(text.starts_with("2 meetings in Yap, newest first:"));
        assert!(text.contains("**Q3 planning sync**"));
        assert!(text.contains("with Priya, Tom"));

        let (err, text) = s.tool("get_meeting", json!({ "id": "1" })).await;
        assert!(!err, "{text}");
        assert!(text.contains("[0:15] Them: Priya here"));

        let (_, text) = s.tool("search_meetings", json!({ "query": "catering" })).await;
        assert!(text.contains("Offsite planning"));
        assert!(text.contains("(transcript page "));

        let (_, text) = s.tool("get_meeting", json!({ "id": 2, "transcript_page": 2 })).await;
        assert!(text.contains("## Transcript, page 2 of"));

        let (err, text) = s.tool("get_meeting", json!({ "id": 3 })).await;
        assert!(err && text.contains("isn't a meeting"));
        let (err, text) = s.tool("get_note", json!({ "id": 3 })).await;
        assert!(!err && text.starts_with("# Groceries\nNote · folder Personal"));
        let (err, text) = s.tool("get_note", json!({ "id": 404 })).await;
        assert!(err && text.contains("no note with id 404"));
        let (_, text) = s.tool("list_folders", json!({})).await;
        assert!(text.contains("- Meetings: 2 notes"));

        // Writes are off: create_note isn't offered, and refuses if called.
        let (err, text) = s.tool("create_note", json!({ "title": "x", "content": "y" })).await;
        assert!(err && text.contains("switched off"));
        // An unknown tool is a protocol error.
        let r = s.request("tools/call", json!({ "name": "get_dictations", "arguments": {} })).await;
        assert_eq!(r["error"]["code"], -32602);
    }

    #[tokio::test]
    async fn writes_appear_only_when_allowed() {
        let bridge = fake(true, true);
        let mut s = Session::start(bridge);
        s.initialize("2025-11-25").await;
        let list = s.request("tools/list", json!({})).await;
        let tools = list["result"]["tools"].as_array().unwrap();
        let create = tools.iter().find(|t| t["name"] == "create_note").expect("offered");
        assert_eq!(create["annotations"]["readOnlyHint"], false);
        let (err, text) = s
            .tool("create_note", json!({ "title": "Ideas list", "content": "- one", "folder": "Ideas" }))
            .await;
        assert!(!err, "{text}");
        assert!(text.contains("note id 77"));
    }

    #[tokio::test]
    async fn yap_closed_reads_as_a_clear_tool_error() {
        let mut s = Session::start(fake(false, false));
        s.initialize("2025-06-18").await;
        // Listing still works (read-only tools), so the app shows the server.
        let list = s.request("tools/list", json!({})).await;
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 6);
        let (err, text) = s.tool("list_meetings", json!({})).await;
        assert!(err);
        assert!(text.contains("Open Yap to let your AI read your notes"));
    }

    #[tokio::test]
    async fn a_2026_client_discovers_and_calls_without_a_handshake() {
        let mut s = Session::start(fake(true, false));
        let meta = json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientInfo": { "name": "modern", "version": "1" },
            "io.modelcontextprotocol/clientCapabilities": {}
        });
        let d = s.request("server/discover", json!({ "_meta": meta })).await;
        let versions = d["result"]["supportedVersions"].as_array().unwrap();
        assert!(versions.contains(&json!("2026-07-28")));
        assert!(versions.contains(&json!("2025-06-18")));
        let list = s.request("tools/list", json!({ "_meta": meta })).await;
        assert_eq!(list["result"]["resultType"], "complete");
        assert_eq!(list["result"]["cacheScope"], "private");
        let r = s
            .request("tools/call", json!({ "_meta": meta, "name": "list_meetings", "arguments": { "limit": 1 } }))
            .await;
        assert_eq!(r["result"]["resultType"], "complete");
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().starts_with("1 meeting in Yap"));
        // A version nobody knows is refused with the ones that work.
        let mut old = meta.clone();
        old["io.modelcontextprotocol/protocolVersion"] = json!("2099-01-01");
        let r = s.request("tools/list", json!({ "_meta": old })).await;
        assert!(r["error"]["data"]["supported"].as_array().unwrap().contains(&json!("2026-07-28")));
    }
}

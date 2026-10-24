//! `--bench …`: **serving benches** for a model on Yap's own llamafile (or
//! any OpenAI-compatible server), measuring how the server is used rather
//! than what the model says — the questions behind the local-AI research
//! (docs/model-evals.md "Serving benches"):
//!
//! - **`stream`**: time to the first token, streamed (SSE, `stream: true`)
//!   vs the whole answer not streamed, on Chat-shaped requests (Chat's own
//!   persona, tool instructions and tool schemas): a short answer, a long
//!   one, a web answer's last step (a `web_search` result ~2.5k tokens long)
//!   and a question that should start with a tool call — and whether the
//!   streamed tool calls put themselves back together as valid JSON. Each
//!   request starts cold (a nonce first in the system prompt), as Chat's
//!   turns do today.
//! - **`layout`**: a four-turn Chat, twice: with the time and the notes
//!   found for each question in the **system prompt** (what `chat_turn`
//!   sends today) and with them in the **last user message** (the system
//!   prompt and tool schemas the same every turn). llama.cpp re-uses a
//!   slot's cache only up to where two prompts first differ — for a hybrid
//!   model like Qwen3.5, only up to a context checkpoint before that — so
//!   this measures the prompt tokens each turn has to process again.
//! - **`embed`**: an embedding model (`--embedding` server): throughput over
//!   note-sized chunks, one query's latency, and retrieval over
//!   `evals/retrieval/notes.json` against Chat's keyword search
//!   (`tools::search_notes`) — recall@1 / @3 / @5.
//!
//! Every request and answer goes to `transcript.jsonl` (`bench_request`,
//! `bench_response`); `--temperature` / `--set` apply here too.

use std::path::Path;
use std::time::Instant;

use serde_json::{json, Value};

use super::cleanup::Endpoint;
use super::record;

pub const BENCHES: &[&str] = &["stream", "layout", "loop", "embed"];

/// Run bench `kind` on one model; its results (JSON) for summary.json and
/// serving.md.
/// `history`: earlier question-and-answer pairs (~450 tokens each) before
/// the layout bench's four turns — a long chat, as on a 32–64K context.
pub async fn run(kind: &str, ep: &Endpoint<'_>, rounds: u32, history: usize) -> Value {
    let result = match kind {
        "stream" => stream_bench(ep, rounds).await,
        "layout" => layout_bench(ep, rounds, history).await,
        "loop" => loop_bench(ep, rounds).await,
        "embed" => embed_bench(ep, rounds).await,
        other => Err(format!("no bench {other}")),
    };
    match result {
        Ok(v) => json!({ "kind": kind, "result": v }),
        Err(e) => {
            println!("  bench {kind} failed: {e}");
            json!({ "kind": kind, "error": e })
        }
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())
}

fn post(client: &reqwest::Client, ep: &Endpoint<'_>, path: &str, body: &Value) -> reqwest::RequestBuilder {
    let mut req = client.post(format!("{}/{}", ep.base_url.trim_end_matches('/'), path)).json(body);
    if !ep.key.is_empty() {
        req = req.bearer_auth(ep.key);
    }
    req
}

/// Chat's system prompt for a tool model with apps and the web on (as
/// `turn::chat_turn` builds it), and its tools.
fn chat_system() -> (String, Value) {
    let mut system = format!("{}\n\nYou have access to tools. ", super::runner::DEFAULT_PERSONA);
    system.push_str(&crate::chat::tools::TOOL_INSTRUCTIONS.join(" "));
    system.push(' ');
    system.push_str(&crate::chat::tools::APP_TOOL_INSTRUCTIONS.join(" "));
    system.push(' ');
    system.push_str(&crate::chat::tools::WEB_TOOL_INSTRUCTIONS.join(" "));
    let mut tools = crate::chat::tools::definitions();
    if let Some(all) = tools.as_array_mut() {
        for more in [crate::chat::tools::app_definitions(), crate::chat::tools::web_definitions()] {
            all.extend(more.as_array().into_iter().flatten().cloned());
        }
    }
    (system, tools)
}

fn time_line(minute: u32) -> String {
    format!(
        "Today is Wednesday 21 October 2026. The user's local time is 14:{minute:02} (UTC+01:00): when a page gives a time in another time zone (ET, PT, CET, UTC…), convert it to the user's time and say which zone you mean."
    )
}

// ── stream ────────────────────────────────────────────────────────────────

/// A web search's result as `web.rs` hands it to the model: numbered hits
/// with their page text (~2.5k tokens).
fn web_result() -> String {
    let pages = [
        ("Announcing Rust 1.91.0", "blog.rust-lang.org", "The Rust team is happy to announce a new version of Rust, 1.91.0. Rust is a programming language empowering everyone to build reliable and efficient software. This release stabilises several long-awaited features. Pattern types let a function say that an integer argument is always within a range, so the compiler can drop bounds checks. The standard library gains new methods on slices and strings, among them a faster split for ASCII text and an iterator over the chunks of a string by byte length that never splits a character. Cargo now resolves dependencies with a newer default resolver that takes the declared minimum Rust version into account, so a project no longer pulls in a crate version that needs a newer compiler than the one it supports. Rustdoc's search is quicker on large workspaces."),
        ("What's new in Rust 1.91: pattern types and more", "infoworld.example", "Rust 1.91 arrived on schedule six weeks after 1.90. The headline is pattern types, which have been in the works for over two years: they let a type carry a restriction such as 'a u32 from 1 to 100', checked when a value is made. Developers also get let chains in more positions, a change to how closures capture disjoint fields in some edge cases, and lints that catch common mistakes with async functions. On Windows, the linker defaults changed for the MSVC target, which shortens link times for big binaries by about a fifth in the team's own tests. The release also includes the usual round of compiler speed-ups: the performance dashboard shows debug builds between 3 and 6 percent faster on common crates."),
        ("Rust 1.91 release notes", "github.com/rust-lang/rust", "Language: stabilise pattern types for integers; allow let chains in while conditions; the never type fallback changes in the 2027 edition preview. Compiler: raise the minimum supported LLVM to 20; add the aarch64-pc-windows-gnullvm target to tier 2 with host tools. Libraries: add slice::split_once and str::split_once_ascii; make Vec::extract_if stable; add f16 and f128 formatting. Cargo: the v3 resolver honours rust-version by default; cargo publish verifies the package in a clean directory by default. Rustdoc: search index compression. Compatibility notes: a few crates relying on unspecified drop order in closures may see a new warning; the windows-gnu target now links against UCRT."),
        ("Rust 1.91 is out — discussion", "reddit.example/r/rust", "People are mostly excited about pattern types. One commenter tried them in an embedded project and removed about forty manual range checks; another pointed out that error messages for range violations still need work. Several replies mention the faster debug builds on Windows. A maintainer of a popular web framework says the resolver change fixed a long-standing issue where users on older compilers got errors after updating dependencies. There is some debate about whether let chains in while conditions make code harder to read. A few people ask when async closures will be stabilised; the answer from the lang team is that it is planned for early next year."),
        ("Upgrading to Rust 1.91", "dev.example", "To upgrade, run rustup update stable. If you maintain a library, check the compatibility notes: the closure capture change can, rarely, alter the order in which fields are dropped. Projects on the windows-gnu target should test linking, since it now uses UCRT. If you rely on the old Cargo resolver behaviour, you can opt out per project with resolver = '2'. The new pattern types are opt-in: existing code is unaffected. For teams pinning a minimum Rust version, the resolver change means fewer surprise breakages when dependencies publish versions that need newer compilers."),
    ];
    let hits: Vec<Value> = pages
        .iter()
        .enumerate()
        .map(|(i, (title, site, text))| {
            json!({ "n": i + 1, "title": title, "url": format!("https://{site}/rust-1-91"), "text": format!("{text} {text}") })
        })
        .collect();
    json!({ "success": true, "query": "Rust 1.91 release what's new", "results": hits, "note": "Cite these as [1], [2]… by their n." })
        .to_string()
}

/// The stream bench's requests: (name, messages after the system prompt).
fn stream_requests() -> Vec<(&'static str, Vec<Value>)> {
    vec![
        ("short answer", vec![json!({ "role": "user", "content": "What's the capital of Australia, and roughly how many people live there?" })]),
        (
            "long answer",
            vec![json!({ "role": "user", "content": "Explain how a heat pump heats a house in winter, step by step, in about 200 words." })],
        ),
        (
            "web answer",
            vec![
                json!({ "role": "user", "content": "What's new in the latest Rust release?" }),
                json!({ "role": "assistant", "content": "", "tool_calls": [{ "id": "call_1", "type": "function", "function": { "name": "web_search", "arguments": "{\"query\":\"Rust 1.91 release what's new\"}" } }] }),
                json!({ "role": "tool", "tool_call_id": "call_1", "content": web_result() }),
            ],
        ),
        (
            "tool call",
            vec![json!({ "role": "user", "content": "Find my note about the launch plan and tell me when it ships." })],
        ),
    ]
}

/// One request's timings, streamed or not.
#[derive(Debug, Default, Clone, serde::Serialize)]
struct Timed {
    total_ms: u64,
    /// The first byte of any delta (content, reasoning or a tool call).
    first_delta_ms: Option<u64>,
    /// The first visible answer text.
    first_content_ms: Option<u64>,
    /// The first tool-call delta.
    first_tool_ms: Option<u64>,
    chunks: u32,
    content_chars: usize,
    /// Tool calls as they came: name and whether the arguments parse.
    tool_calls: Vec<(String, bool)>,
    prompt_n: u64,
    prompt_ms: f64,
    predicted_n: u64,
    predicted_ms: f64,
    error: Option<String>,
}

fn timings_into(t: &mut Timed, timings: &Value) {
    if timings.is_object() {
        t.prompt_n = timings["prompt_n"].as_u64().unwrap_or(0);
        t.prompt_ms = timings["prompt_ms"].as_f64().unwrap_or(0.0);
        t.predicted_n = timings["predicted_n"].as_u64().unwrap_or(0);
        t.predicted_ms = timings["predicted_ms"].as_f64().unwrap_or(0.0);
    }
}

async fn send_whole(client: &reqwest::Client, ep: &Endpoint<'_>, body: &Value) -> Timed {
    let mut t = Timed::default();
    let t0 = Instant::now();
    let res = post(client, ep, "chat/completions", body).send().await;
    let v: Result<Value, String> = match res {
        Ok(r) if r.status().is_success() => r.json::<Value>().await.map_err(|e| e.to_string()),
        Ok(r) => Err(format!("HTTP {}: {}", r.status(), r.text().await.unwrap_or_default().chars().take(300).collect::<String>())),
        Err(e) => Err(e.to_string()),
    };
    t.total_ms = t0.elapsed().as_millis() as u64;
    match v {
        Ok(v) => {
            let msg = &v["choices"][0]["message"];
            t.content_chars = msg["content"].as_str().map_or(0, |s| s.chars().count());
            t.tool_calls = msg["tool_calls"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|c| {
                    let args = &c["function"]["arguments"];
                    let ok = args.is_object() || args.as_str().is_some_and(|s| serde_json::from_str::<Value>(s).is_ok());
                    (c["function"]["name"].as_str().unwrap_or("").to_string(), ok)
                })
                .collect();
            timings_into(&mut t, &v["timings"]);
            record::write("bench_response", json!({ "stream": false, "ms": t.total_ms, "body": v }));
        }
        Err(e) => {
            record::write("bench_response", json!({ "stream": false, "ms": t.total_ms, "error": e }));
            t.error = Some(e);
        }
    }
    t
}

/// What an SSE stream's `data:` lines say, put back together.
#[derive(Debug, Default)]
struct Assembled {
    content: String,
    /// Tool calls by index: (name, arguments as streamed).
    tools: Vec<(String, String)>,
}

impl Assembled {
    /// Fold one chunk's delta in; what it carried (content, tool, other).
    fn add(&mut self, chunk: &Value) -> (bool, bool, bool) {
        let delta = &chunk["choices"][0]["delta"];
        let mut seen = (false, false, false);
        if let Some(c) = delta["content"].as_str().filter(|c| !c.is_empty()) {
            self.content.push_str(c);
            seen.0 = true;
        }
        for call in delta["tool_calls"].as_array().into_iter().flatten() {
            let i = call["index"].as_u64().unwrap_or(0) as usize;
            while self.tools.len() <= i {
                self.tools.push((String::new(), String::new()));
            }
            if let Some(n) = call["function"]["name"].as_str() {
                self.tools[i].0.push_str(n);
            }
            if let Some(a) = call["function"]["arguments"].as_str() {
                self.tools[i].1.push_str(a);
            }
            seen.1 = true;
        }
        if delta["reasoning_content"].as_str().is_some_and(|r| !r.is_empty()) {
            seen.2 = true;
        }
        seen
    }
}

/// Split a growing SSE buffer into complete `data:` payloads, leaving the
/// rest in `buf`. Pure.
fn take_events(buf: &mut String) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(end) = buf.find("\n\n") {
        let event: String = buf.drain(..end + 2).collect();
        for line in event.lines() {
            if let Some(data) = line.strip_prefix("data:") {
                out.push(data.trim().to_string());
            }
        }
    }
    out
}

async fn send_streamed(client: &reqwest::Client, ep: &Endpoint<'_>, body: &Value) -> Timed {
    let mut t = Timed::default();
    let t0 = Instant::now();
    let mut body = body.clone();
    body["stream"] = json!(true);
    body["stream_options"] = json!({ "include_usage": true });
    let mut resp = match post(client, ep, "chat/completions", &body).send().await {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            t.error = Some(format!("HTTP {}: {}", r.status(), r.text().await.unwrap_or_default().chars().take(300).collect::<String>()));
            t.total_ms = t0.elapsed().as_millis() as u64;
            return t;
        }
        Err(e) => {
            t.error = Some(e.to_string());
            return t;
        }
    };
    let mut buf = String::new();
    let mut got = Assembled::default();
    let mut last = Value::Null;
    loop {
        let chunk = match resp.chunk().await {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(e) => {
                t.error = Some(e.to_string());
                break;
            }
        };
        buf.push_str(&String::from_utf8_lossy(&chunk));
        for data in take_events(&mut buf) {
            if data == "[DONE]" {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(&data) else { continue };
            let ms = t0.elapsed().as_millis() as u64;
            t.chunks += 1;
            let (content, tool, other) = got.add(&v);
            if (content || tool || other) && t.first_delta_ms.is_none() {
                t.first_delta_ms = Some(ms);
            }
            if content && t.first_content_ms.is_none() {
                t.first_content_ms = Some(ms);
            }
            if tool && t.first_tool_ms.is_none() {
                t.first_tool_ms = Some(ms);
            }
            if v["timings"].is_object() {
                last = v.clone();
            }
        }
    }
    t.total_ms = t0.elapsed().as_millis() as u64;
    t.content_chars = got.content.chars().count();
    t.tool_calls = got.tools.iter().map(|(n, a)| (n.clone(), serde_json::from_str::<Value>(a).is_ok())).collect();
    timings_into(&mut t, &last["timings"]);
    record::write(
        "bench_response",
        json!({ "stream": true, "ms": t.total_ms, "chunks": t.chunks, "first_content_ms": t.first_content_ms, "first_tool_ms": t.first_tool_ms, "content": got.content, "tool_calls": got.tools, "timings": last["timings"], "error": t.error }),
    );
    t
}

fn median(mut v: Vec<u64>) -> Option<u64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    Some(v[v.len() / 2])
}

async fn stream_bench(ep: &Endpoint<'_>, rounds: u32) -> Result<Value, String> {
    let client = client()?;
    let (system, tools) = chat_system();
    let mut rows = Vec::new();
    for (name, rest) in stream_requests() {
        let mut whole = Vec::new();
        let mut streamed = Vec::new();
        for round in 0..rounds {
            for stream in [false, true] {
                // A nonce first: every request starts cold, as Chat's turns
                // do today (their system prompt changes each turn).
                let nonce = format!("[bench {name} {round} {stream} {}]", chrono::Local::now().timestamp_millis());
                let mut messages = vec![json!({ "role": "system", "content": format!("{nonce}\n{system} {}", time_line(5)) })];
                messages.extend(rest.iter().cloned());
                let mut body = json!({ "model": ep.model, "messages": messages, "tools": tools, "temperature": 0.5 });
                record::adjust_body(&mut body);
                record::write("bench_request", json!({ "bench": "stream", "request": name, "round": round, "stream": stream, "body": body }));
                let t = if stream { send_streamed(&client, ep, &body).await } else { send_whole(&client, ep, &body).await };
                if stream {
                    streamed.push(t);
                } else {
                    whole.push(t);
                }
            }
        }
        let errors: Vec<String> = whole.iter().chain(streamed.iter()).filter_map(|t| t.error.clone()).collect();
        let row = json!({
            "request": name,
            "rounds": rounds,
            "whole_ms": median(whole.iter().map(|t| t.total_ms).collect()),
            "streamed_total_ms": median(streamed.iter().map(|t| t.total_ms).collect()),
            "first_content_ms": median(streamed.iter().filter_map(|t| t.first_content_ms).collect()),
            "first_tool_ms": median(streamed.iter().filter_map(|t| t.first_tool_ms).collect()),
            "first_delta_ms": median(streamed.iter().filter_map(|t| t.first_delta_ms).collect()),
            "prompt_tokens": median(streamed.iter().map(|t| t.prompt_n).collect()),
            "prompt_ms": median(streamed.iter().map(|t| t.prompt_ms.round() as u64).collect()),
            "gen_tokens": median(streamed.iter().map(|t| t.predicted_n).collect()),
            "content_chars": median(streamed.iter().map(|t| t.content_chars as u64).collect()),
            "whole_tool_calls": whole.iter().map(|t| t.tool_calls.clone()).collect::<Vec<_>>(),
            "streamed_tool_calls": streamed.iter().map(|t| t.tool_calls.clone()).collect::<Vec<_>>(),
            "streamed_bad_tool_args": streamed.iter().flat_map(|t| t.tool_calls.iter()).filter(|(_, ok)| !ok).count(),
            "errors": errors,
        });
        println!(
            "  {:<12} whole {:>6?} ms  streamed: first words {:>6?} ms, first tool call {:>6?} ms, done {:>6?} ms  (prompt {:?} tok)",
            name, row["whole_ms"], row["first_content_ms"], row["first_tool_ms"], row["streamed_total_ms"], row["prompt_tokens"]
        );
        rows.push(row);
    }
    Ok(json!({ "requests": rows }))
}

// ── layout ────────────────────────────────────────────────────────────────

/// Notes the "search" finds for each turn (each a `<note>` like
/// `turn::rag_context`'s, ~500 characters).
const NOTES: &[(&str, &str)] = &[
    ("Launch plan", "The beta ships on Friday 14 November if the installer bug is fixed by Wednesday. Dave owns the installer fix; Priya checks the signing certificate on Tuesday. Marketing wants screenshots of the new Yap bar and the meeting notepad by Monday, and the release notes should lead with offline transcription. Support needs a FAQ about GPU drivers and the first-run model download, which is 600 MB. We agreed to hold the Mac build until December."),
    ("Kitchen renovation", "Quote from Harlow Kitchens: £14,200 including fitting, worktops in quartz, eight weeks lead time. The second quote from a local joiner was £11,800 but with laminate worktops and no fitting of appliances. Electrician needs two days to move the sockets for the island. Ask about the extractor duct running through the ceiling and whether the boiler cupboard can stay where it is. Deposit of 25% due on signing."),
    ("Running plan", "Week 6 of the half-marathon plan: Tuesday 8 km easy, Thursday intervals 6 x 800 m at 4:30/km with 2 minutes rest, Saturday long run 16 km at 6:00/km. Left knee felt tight after the long run last week; foam roll and keep the cadence up. Race is on 7 December in Cardiff; target time under 1:55. Buy new trainers before week 8, the current pair has done 700 km."),
    ("Book club", "Next book: The Remains of the Day. Meeting at Sarah's on the 28th at 7:30. Last time we discussed Klara and the Sun and most people preferred the first half. Tom suggested we read something non-fiction in January; ideas were Braiding Sweetgrass and The Wager. Bring snacks; Sarah is vegetarian. Remember to return the library copy of the last book, it's overdue."),
    ("Car service", "The Golf is due its 60,000-mile service. Garage quoted £340 for the major service plus £95 for front brake pads. MOT expires on 3 January, so book both together in December. The tyre pressure warning came on twice in the cold; check the rear left valve. Insurance renews on 19 November; get comparison quotes, last year's was £612."),
    ("Garden ideas", "Plant tulip bulbs in November, about 60 of them along the front path, mixing Queen of Night and Ballerina. Move the hydrangea to the shady corner in early spring. The water butt is leaking at the tap; replace the tap. Ideas for next year: raised beds for vegetables near the shed, a small pond for wildlife, and a bench under the apple tree. Prune the apple tree in February."),
    ("Holiday ideas", "Options for Easter: Lisbon (flights £180 each, four nights, trams and pastel de nata), the Lake District (cottage near Keswick, £620 for the week, walking), or Copenhagen (more expensive, design museums, bike everywhere). Kids want somewhere with a beach. Check passport expiry dates before booking: one runs out in June. Budget around £2,000 in total."),
    ("Team retro", "What went well: the new release checklist, fewer late-night deploys, faster code review. What didn't: flaky end-to-end tests on CI, unclear ownership of the support inbox, and too many meetings on Tuesdays. Actions: Priya to quarantine the flaky tests, Dave to draft a support rota, everyone to move status updates to the written stand-up thread."),
];

fn rag_block(turn: usize) -> String {
    (0..5)
        .map(|i| {
            let (title, text) = NOTES[(turn * 3 + i) % NOTES.len()];
            format!("<note id=\"{}\" title=\"{title}\">\n{text}\n</note>", (turn * 3 + i) % NOTES.len() + 1)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

const LAYOUT_TURNS: &[(&str, &str)] = &[
    ("When does the beta ship, and what has to happen first?", "The beta ships on Friday 14 November, as long as Dave's installer fix lands by Wednesday. Priya also checks the signing certificate on Tuesday, and marketing wants screenshots by Monday."),
    ("What did the two kitchen quotes come to?", "Harlow Kitchens quoted £14,200 with fitting and quartz worktops; the local joiner quoted £11,800 with laminate worktops and no appliance fitting. The deposit is 25% on signing."),
    ("What's my long run this week?", "This week's long run is 16 km on Saturday at about 6:00 per km. Keep an eye on your left knee: foam roll and keep your cadence up."),
    ("Remind me what's left to do for the car.", "Book the 60,000-mile service and the MOT together in December (the MOT runs out on 3 January), check the rear left tyre valve, and get insurance quotes before it renews on 19 November."),
];

/// One layout's conversation: per turn, the request's prompt tokens, those
/// processed (the rest came from the slot's cache), the prompt time and the
/// call's wall time.
/// Earlier turns of a long chat: a question and a ~450-token answer each.
fn earlier_pairs(n: usize) -> Vec<Value> {
    let mut out = Vec::new();
    for i in 0..n {
        let (title, text) = NOTES[i % NOTES.len()];
        out.push(json!({ "role": "user", "content": format!("Tell me everything in my note \"{title}\" ({}), in full.", i + 1) }));
        out.push(json!({ "role": "assistant", "content": format!("Here is everything in \"{title}\": {text} {text} {text}") }));
    }
    out
}

async fn layout_conversation(
    client: &reqwest::Client,
    ep: &Endpoint<'_>,
    stable: bool,
    nonce: &str,
    history: usize,
) -> Result<Vec<Value>, String> {
    let (base, tools) = chat_system();
    let mut out = Vec::new();
    for k in 0..LAYOUT_TURNS.len() {
        let rag = rag_block(k);
        let time = time_line(2 + k as u32 * 3);
        let lead = "Below are notes from the user's library that may be relevant. Reference them naturally if they help answer the question.";
        let system = if stable {
            format!("{nonce}\n{base}")
        } else {
            format!("{nonce}\n{base} {time}\n\n{lead}\n\n{rag}")
        };
        let mut messages = vec![json!({ "role": "system", "content": system })];
        messages.extend(earlier_pairs(history));
        for (q, a) in &LAYOUT_TURNS[..k] {
            messages.push(json!({ "role": "user", "content": q }));
            messages.push(json!({ "role": "assistant", "content": a }));
        }
        let question = LAYOUT_TURNS[k].0;
        let last = if stable {
            format!("<context>\n{time}\n\n{lead}\n\n{rag}\n</context>\n\n{question}")
        } else {
            question.to_string()
        };
        messages.push(json!({ "role": "user", "content": last }));
        // One token: the prompt's processing is what's measured.
        let mut body = json!({ "model": ep.model, "messages": messages, "tools": tools, "temperature": 0.5, "max_tokens": 1 });
        record::adjust_body(&mut body);
        record::write("bench_request", json!({ "bench": "layout", "stable": stable, "turn": k + 1, "body": body }));
        let t0 = Instant::now();
        let res = post(client, ep, "chat/completions", &body).send().await.map_err(|e| e.to_string())?;
        let status = res.status();
        let v: Value = res.json().await.map_err(|e| e.to_string())?;
        let ms = t0.elapsed().as_millis() as u64;
        record::write("bench_response", json!({ "bench": "layout", "stable": stable, "turn": k + 1, "ms": ms, "body": v }));
        if !status.is_success() {
            return Err(format!("HTTP {status}: {}", v.to_string().chars().take(300).collect::<String>()));
        }
        let prompt = v["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
        let processed = v["timings"]["prompt_n"].as_u64().unwrap_or(0);
        out.push(json!({
            "turn": k + 1,
            "prompt_tokens": prompt,
            "processed": processed,
            "cached": prompt.saturating_sub(processed),
            "prompt_ms": v["timings"]["prompt_ms"].as_f64().map(|x| x.round()),
            "ms": ms,
        }));
    }
    Ok(out)
}

async fn layout_bench(ep: &Endpoint<'_>, rounds: u32, history: usize) -> Result<Value, String> {
    let client = client()?;
    let mut runs = Vec::new();
    for round in 0..rounds {
        for stable in [false, true] {
            let nonce = format!("[bench layout {round} {stable} {}]", chrono::Local::now().timestamp_millis());
            let turns = layout_conversation(&client, ep, stable, &nonce, history).await?;
            for t in &turns {
                println!(
                    "  round {} {:<6} turn {}: prompt {:>5} tok, processed {:>5} ({:>5} cached), prompt {:>6?} ms, call {:>6} ms",
                    round + 1,
                    if stable { "stable" } else { "today" },
                    t["turn"],
                    t["prompt_tokens"],
                    t["processed"],
                    t["cached"],
                    t["prompt_ms"],
                    t["ms"]
                );
            }
            runs.push(json!({ "round": round + 1, "layout": if stable { "stable" } else { "today" }, "turns": turns }));
        }
    }
    // Per layout and turn: the median over the rounds.
    let mut summary = Vec::new();
    for layout in ["today", "stable"] {
        for k in 1..=LAYOUT_TURNS.len() {
            let pick = |key: &str| -> Option<u64> {
                median(
                    runs.iter()
                        .filter(|r| r["layout"] == json!(layout))
                        .filter_map(|r| r["turns"].as_array()?.iter().find(|t| t["turn"] == json!(k)).and_then(|t| t[key].as_f64()))
                        .map(|x| x.round() as u64)
                        .collect(),
                )
            };
            summary.push(json!({
                "layout": layout, "turn": k,
                "prompt_tokens": pick("prompt_tokens"), "processed": pick("processed"), "cached": pick("cached"),
                "prompt_ms": pick("prompt_ms"), "ms": pick("ms"),
            }));
        }
    }
    Ok(json!({ "runs": runs, "summary": summary, "history_pairs": history }))
}

// ── loop ──────────────────────────────────────────────────────────────────

/// The tool loop's second step after a web search, as `tools::run_tool_loop`
/// sends it, in variants: which tools each step offers changes the prompt's
/// start (a template like Qwen3.5's writes the tool schemas first), so a
/// step that offers other tools than the one before can't re-use its cache.
/// (name, step 1's tools, step 1's `tool_choice`, step 2's tools).
fn loop_variants() -> Vec<(&'static str, &'static str, Option<Value>, &'static str)> {
    vec![
        // Today: every tool, then — web content in the turn — no action tools.
        ("search, then actions dropped (today)", "all", None, "no-actions"),
        ("search, tools kept (refused instead)", "all", None, "all"),
        // Today for a local model and a question that needs the web.
        ("must search: web_search only + required (today)", "web_search", Some(json!("required")), "no-actions"),
        ("must search: all tools + required", "all", Some(json!("required")), "all"),
        (
            "must search: all tools + named web_search",
            "all",
            Some(json!({ "type": "function", "function": { "name": "web_search" } })),
            "all",
        ),
    ]
}

fn tool_set(which: &str, all: &Value) -> Value {
    match which {
        "no-actions" => crate::chat::tools::without_actions(all),
        "web_search" => Value::Array(all.as_array().into_iter().flatten().filter(|t| t["function"]["name"] == "web_search").cloned().collect()),
        _ => all.clone(),
    }
}

async fn loop_bench(ep: &Endpoint<'_>, rounds: u32) -> Result<Value, String> {
    let client = client()?;
    let (base, all) = chat_system();
    let mut runs = Vec::new();
    for round in 0..rounds {
        for (name, first_tools, choice, second_tools) in loop_variants() {
            let nonce = format!("[bench loop {round} {name} {}]", chrono::Local::now().timestamp_millis());
            let system = format!("{nonce}\n{base} {}", time_line(7));
            let question = json!({ "role": "user", "content": "What's new in the latest Rust release?" });
            let mut step1 = json!({
                "model": ep.model,
                "messages": [{ "role": "system", "content": system }, question],
                "tools": tool_set(first_tools, &all),
                "temperature": 0.5,
                "max_tokens": 48,
            });
            if let Some(c) = &choice {
                step1["tool_choice"] = c.clone();
            }
            record::adjust_body(&mut step1);
            record::write("bench_request", json!({ "bench": "loop", "variant": name, "step": 1, "body": step1 }));
            let t0 = Instant::now();
            let res = post(&client, ep, "chat/completions", &step1).send().await.map_err(|e| e.to_string())?;
            let status1 = res.status();
            let v1: Value = res.json().await.unwrap_or(Value::Null);
            let ms1 = t0.elapsed().as_millis() as u64;
            record::write("bench_response", json!({ "bench": "loop", "variant": name, "step": 1, "ms": ms1, "body": v1 }));
            let called: Vec<String> = v1["choices"][0]["message"]["tool_calls"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| c["function"]["name"].as_str().map(str::to_string))
                .collect();
            // Step 2: the search's result in, as the loop sends it.
            let step2 = {
                let mut b = json!({
                    "model": ep.model,
                    "messages": [
                        { "role": "system", "content": system },
                        question,
                        { "role": "assistant", "content": null, "tool_calls": [{ "id": "call_1", "type": "function", "function": { "name": "web_search", "arguments": "{\"query\":\"Rust 1.91 release what's new\"}" } }] },
                        { "role": "tool", "tool_call_id": "call_1", "content": web_result() },
                    ],
                    "tools": tool_set(second_tools, &all),
                    "temperature": 0.5,
                    "max_tokens": 1,
                });
                record::adjust_body(&mut b);
                b
            };
            record::write("bench_request", json!({ "bench": "loop", "variant": name, "step": 2, "body": step2 }));
            let t0 = Instant::now();
            let res = post(&client, ep, "chat/completions", &step2).send().await.map_err(|e| e.to_string())?;
            let v2: Value = res.json().await.unwrap_or(Value::Null);
            let ms2 = t0.elapsed().as_millis() as u64;
            record::write("bench_response", json!({ "bench": "loop", "variant": name, "step": 2, "ms": ms2, "body": v2 }));
            let prompt = v2["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
            let processed = v2["timings"]["prompt_n"].as_u64().unwrap_or(0);
            let row = json!({
                "round": round + 1,
                "variant": name,
                "step1_status": status1.as_u16(),
                "step1_called": called,
                "step1_prompt_ms": v1["timings"]["prompt_ms"].as_f64().map(|x| x.round()),
                "step1_ms": ms1,
                "step2_prompt_tokens": prompt,
                "step2_processed": processed,
                "step2_cached": prompt.saturating_sub(processed),
                "step2_prompt_ms": v2["timings"]["prompt_ms"].as_f64().map(|x| x.round()),
                "step2_ms": ms2,
            });
            println!(
                "  round {} {:<48} step 1 HTTP {} called {:?}; step 2: prompt {:>5} tok, processed {:>5}, prompt {:>6?} ms, call {:>5} ms",
                round + 1,
                name,
                status1.as_u16(),
                row["step1_called"],
                prompt,
                processed,
                row["step2_prompt_ms"],
                ms2
            );
            runs.push(row);
        }
    }
    let summary: Vec<Value> = loop_variants()
        .iter()
        .map(|(name, ..)| {
            let pick = |key: &str| median(runs.iter().filter(|r| r["variant"] == json!(name)).filter_map(|r| r[key].as_f64()).map(|x| x.round() as u64).collect());
            let called: Vec<Value> = runs.iter().filter(|r| r["variant"] == json!(name)).map(|r| r["step1_called"].clone()).collect();
            json!({
                "variant": name,
                "step1_status": runs.iter().find(|r| r["variant"] == json!(name)).map(|r| r["step1_status"].clone()),
                "step1_called": called,
                "step2_prompt_tokens": pick("step2_prompt_tokens"),
                "step2_processed": pick("step2_processed"),
                "step2_cached": pick("step2_cached"),
                "step2_prompt_ms": pick("step2_prompt_ms"),
                "step2_ms": pick("step2_ms"),
            })
        })
        .collect();
    Ok(json!({ "runs": runs, "summary": summary }))
}

// ── embed ─────────────────────────────────────────────────────────────────

/// `evals/retrieval/notes.json`: notes and questions with the note that
/// answers each.
#[derive(Debug, serde::Deserialize)]
pub struct Retrieval {
    pub notes: Vec<RNote>,
    pub queries: Vec<RQuery>,
}

#[derive(Debug, serde::Deserialize)]
pub struct RNote {
    pub title: String,
    pub content: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct RQuery {
    pub q: String,
    /// The title of the note that answers it.
    pub note: String,
}

pub fn load_retrieval(path: &Path) -> Result<Retrieval, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let r: Retrieval = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    for q in &r.queries {
        if !r.notes.iter().any(|n| n.title == q.note) {
            return Err(format!("{}: query \"{}\" names no note \"{}\"", path.display(), q.q, q.note));
        }
    }
    Ok(r)
}

/// The prefixes a model's card asks for (query, document), by its file name.
fn embed_prefixes(model: &str) -> (&'static str, &'static str) {
    let m = model.to_lowercase();
    if m.contains("embeddinggemma") || m.contains("embedding-gemma") {
        ("task: search result | query: ", "title: none | text: ")
    } else if m.contains("qwen3-embedding") || m.contains("qwen3_embedding") {
        ("Instruct: Given a question, retrieve the note that answers it\nQuery: ", "")
    } else if m.contains("nomic") {
        ("search_query: ", "search_document: ")
    } else if m.contains("bge") || m.contains("arctic") {
        ("Represent this sentence for searching relevant passages: ", "")
    } else if m.contains("e5") {
        ("query: ", "passage: ")
    } else {
        ("", "")
    }
}

async fn embed(client: &reqwest::Client, ep: &Endpoint<'_>, input: &[String]) -> Result<Vec<Vec<f32>>, String> {
    let body = json!({ "model": ep.model, "input": input });
    let res = post(client, ep, "embeddings", &body).send().await.map_err(|e| e.to_string())?;
    let status = res.status();
    let v: Value = res.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("HTTP {status}: {}", v.to_string().chars().take(300).collect::<String>()));
    }
    let mut data: Vec<(u64, Vec<f32>)> = v["data"]
        .as_array()
        .ok_or("no data in the embeddings answer")?
        .iter()
        .map(|d| {
            (
                d["index"].as_u64().unwrap_or(0),
                d["embedding"].as_array().into_iter().flatten().filter_map(|x| x.as_f64().map(|f| f as f32)).collect(),
            )
        })
        .collect();
    data.sort_by_key(|(i, _)| *i);
    Ok(data.into_iter().map(|(_, e)| e).collect())
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

/// Recall@k: the share of queries whose note is in the first k. Pure.
fn recall_at(ranks: &[Option<usize>], k: usize) -> f64 {
    if ranks.is_empty() {
        return 0.0;
    }
    let hit = ranks.iter().filter(|r| r.is_some_and(|r| r < k)).count();
    (hit as f64 / ranks.len() as f64 * 1000.0).round() / 1000.0
}

async fn embed_bench(ep: &Endpoint<'_>, rounds: u32) -> Result<Value, String> {
    let client = client()?;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/retrieval/notes.json");
    let set = load_retrieval(&path)?;
    let (qp, dp) = embed_prefixes(ep.model);

    // Throughput: note-sized chunks (~350 words), 16 at a time.
    let chunk: String = NOTES.iter().map(|(_, t)| *t).collect::<Vec<_>>().join(" ");
    let chunks: Vec<String> = (0..64).map(|i| format!("{dp}{i}. {chunk}")).collect();
    let t0 = Instant::now();
    let mut dims = 0;
    for batch in chunks.chunks(16) {
        let e = embed(&client, ep, batch).await?;
        dims = e.first().map_or(0, |v| v.len());
    }
    let chunks_ms = t0.elapsed().as_millis() as u64;

    // One short query, as a search box would send it.
    let mut single = Vec::new();
    for i in 0..(10 * rounds) {
        let t = Instant::now();
        embed(&client, ep, &[format!("{qp}when is the dentist appointment {i}")]).await?;
        single.push(t.elapsed().as_millis() as u64);
    }

    // Retrieval: embeddings vs Chat's keyword search over the same notes.
    let docs: Vec<String> = set.notes.iter().map(|n| format!("{dp}{}\n{}", n.title, n.content)).collect();
    let mut doc_vecs = Vec::new();
    for batch in docs.chunks(16) {
        doc_vecs.extend(embed(&client, ep, batch).await?);
    }
    crate::notes::eval_reset();
    let mut ids = Vec::new();
    for n in &set.notes {
        ids.push(crate::notes::create(&n.title, &n.content, "manual", "Personal").id);
    }
    let mut vec_ranks = Vec::new();
    let mut kw_ranks = Vec::new();
    let mut misses = Vec::new();
    for q in &set.queries {
        let want = set.notes.iter().position(|n| n.title == q.note).unwrap_or(0);
        let qv = embed(&client, ep, &[format!("{qp}{}", q.q)]).await?.pop().unwrap_or_default();
        let mut scored: Vec<(usize, f32)> = doc_vecs.iter().enumerate().map(|(i, d)| (i, cosine(&qv, d))).collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        let vr = scored.iter().position(|(i, _)| *i == want);
        vec_ranks.push(vr);
        let found = crate::chat::tools::search_notes(&q.q, 5);
        let kr = found.iter().position(|(_, n)| n.id == ids[want]);
        kw_ranks.push(kr);
        if vr != Some(0) || kr != Some(0) {
            misses.push(json!({ "q": q.q, "note": q.note, "vector_rank": vr.map(|r| r + 1), "keyword_rank": kr.map(|r| r + 1), "vector_top": set.notes[scored[0].0].title }));
        }
    }
    crate::notes::eval_reset();
    let result = json!({
        "dims": dims,
        "chunks": chunks.len(),
        "chunk_words": chunk.split_whitespace().count(),
        "chunks_ms": chunks_ms,
        "chunks_per_s": (chunks.len() as f64 / (chunks_ms.max(1) as f64 / 1000.0) * 10.0).round() / 10.0,
        "query_ms_median": median(single.clone()),
        "query_ms_max": single.iter().max(),
        "notes": set.notes.len(),
        "queries": set.queries.len(),
        "vector_recall": { "at1": recall_at(&vec_ranks, 1), "at3": recall_at(&vec_ranks, 3), "at5": recall_at(&vec_ranks, 5) },
        "keyword_recall": { "at1": recall_at(&kw_ranks, 1), "at3": recall_at(&kw_ranks, 3), "at5": recall_at(&kw_ranks, 5) },
        "misses": misses,
        "prefixes": { "query": qp, "document": dp },
    });
    println!(
        "  {} dims; {} chunks of ~{} words in {} ms; a query {:?} ms; recall@1/3/5 vectors {} / {} / {}, keywords {} / {} / {}",
        dims,
        chunks.len(),
        result["chunk_words"],
        chunks_ms,
        result["query_ms_median"],
        result["vector_recall"]["at1"],
        result["vector_recall"]["at3"],
        result["vector_recall"]["at5"],
        result["keyword_recall"]["at1"],
        result["keyword_recall"]["at3"],
        result["keyword_recall"]["at5"],
    );
    Ok(result)
}

// ── serving.md ────────────────────────────────────────────────────────────

pub fn markdown(models: &[(String, Value)]) -> String {
    let mut md = String::from("# Serving benches\n");
    let ms = |v: &Value| v.as_u64().map_or("—".to_string(), |x| x.to_string());
    for (label, r) in models {
        let kind = r["kind"].as_str().unwrap_or("");
        md.push_str(&format!("\n## {label}: {kind}\n\n"));
        if let Some(e) = r["error"].as_str() {
            md.push_str(&format!("Failed: {e}\n"));
            continue;
        }
        let res = &r["result"];
        match kind {
            "stream" => {
                md.push_str("Medians (ms). Every request cold (its system prompt new), as Chat's turns are today.\n\n| Request | Prompt tok | Prompt ms | Not streamed: done | Streamed: first words | Streamed: first tool call | Streamed: done | Gen tok | Bad streamed tool args | Errors |\n|---|---|---|---|---|---|---|---|---|---|\n");
                for q in res["requests"].as_array().into_iter().flatten() {
                    md.push_str(&format!(
                        "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                        q["request"].as_str().unwrap_or(""),
                        ms(&q["prompt_tokens"]),
                        ms(&q["prompt_ms"]),
                        ms(&q["whole_ms"]),
                        ms(&q["first_content_ms"]),
                        ms(&q["first_tool_ms"]),
                        ms(&q["streamed_total_ms"]),
                        ms(&q["gen_tokens"]),
                        q["streamed_bad_tool_args"],
                        q["errors"].as_array().map_or(0, |a| a.len()),
                    ));
                }
            }
            "layout" => {
                md.push_str("Medians over the rounds. *today*: the time and the notes found in the system prompt; *stable*: in the last user message.\n\n| Layout | Turn | Prompt tok | Processed | From cache | Prompt ms | Call ms |\n|---|---|---|---|---|---|---|\n");
                for t in res["summary"].as_array().into_iter().flatten() {
                    md.push_str(&format!(
                        "| {} | {} | {} | {} | {} | {} | {} |\n",
                        t["layout"].as_str().unwrap_or(""),
                        t["turn"],
                        ms(&t["prompt_tokens"]),
                        ms(&t["processed"]),
                        ms(&t["cached"]),
                        ms(&t["prompt_ms"]),
                        ms(&t["ms"]),
                    ));
                }
            }
            "loop" => {
                md.push_str("A web answer's second step (the search's result in), after a first step that offered these tools. Medians over the rounds.\n\n| Variant | Step 1 HTTP | Step 1 called | Step 2 prompt tok | Processed | From cache | Prompt ms | Call ms |\n|---|---|---|---|---|---|---|---|\n");
                for t in res["summary"].as_array().into_iter().flatten() {
                    md.push_str(&format!(
                        "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
                        t["variant"].as_str().unwrap_or(""),
                        t["step1_status"],
                        t["step1_called"],
                        ms(&t["step2_prompt_tokens"]),
                        ms(&t["step2_processed"]),
                        ms(&t["step2_cached"]),
                        ms(&t["step2_prompt_ms"]),
                        ms(&t["step2_ms"]),
                    ));
                }
            }
            "embed" => {
                md.push_str(&format!(
                    "{} dimensions. {} chunks of ~{} words in {} ms ({} chunks/s); one short query {} ms (median), {} ms (max).\n\nRetrieval over {} notes, {} questions (recall@1 / @3 / @5): **vectors {} / {} / {}**, keywords (Chat's `search_notes`) {} / {} / {}.\n\n",
                    res["dims"], res["chunks"], res["chunk_words"], res["chunks_ms"], res["chunks_per_s"], ms(&res["query_ms_median"]), ms(&res["query_ms_max"]),
                    res["notes"], res["queries"],
                    res["vector_recall"]["at1"], res["vector_recall"]["at3"], res["vector_recall"]["at5"],
                    res["keyword_recall"]["at1"], res["keyword_recall"]["at3"], res["keyword_recall"]["at5"],
                ));
                md.push_str("| Question | Note | Vector rank | Keyword rank | Vectors' first |\n|---|---|---|---|---|\n");
                for m in res["misses"].as_array().into_iter().flatten() {
                    md.push_str(&format!(
                        "| {} | {} | {} | {} | {} |\n",
                        m["q"].as_str().unwrap_or(""),
                        m["note"].as_str().unwrap_or(""),
                        ms(&m["vector_rank"]),
                        ms(&m["keyword_rank"]),
                        m["vector_top"].as_str().unwrap_or(""),
                    ));
                }
            }
            _ => {}
        }
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_events_split_and_assemble() {
        let mut buf = String::from("data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"search_notes\",\"arguments\":\"{\\\"query\\\":\"}}]}}]}\n\ndata: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"launch\\\"}\"}}]}}]}\n\ndata: [DO");
        let events = take_events(&mut buf);
        assert_eq!(events.len(), 2);
        assert_eq!(buf, "data: [DO");
        let mut a = Assembled::default();
        for e in &events {
            let v: Value = serde_json::from_str(e).unwrap();
            assert_eq!(a.add(&v), (false, true, false));
        }
        assert_eq!(a.tools, vec![("search_notes".to_string(), "{\"query\":\"launch\"}".to_string())]);
        assert!(serde_json::from_str::<Value>(&a.tools[0].1).is_ok());
        let mut a = Assembled::default();
        assert_eq!(a.add(&json!({ "choices": [{ "delta": { "content": "Hi" } }] })), (true, false, false));
        assert_eq!(a.content, "Hi");
    }

    #[test]
    fn recall_and_cosine() {
        assert_eq!(recall_at(&[Some(0), Some(2), None, Some(1)], 1), 0.25);
        assert_eq!(recall_at(&[Some(0), Some(2), None, Some(1)], 3), 0.75);
        assert_eq!(recall_at(&[], 3), 0.0);
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
    }

    #[test]
    fn prefixes_follow_the_model_card() {
        assert_eq!(embed_prefixes("embeddinggemma-300M-Q8_0").1, "title: none | text: ");
        assert!(embed_prefixes("Qwen3-Embedding-0.6B-Q8_0").0.starts_with("Instruct:"));
        assert_eq!(embed_prefixes("nomic-embed-text-v1.5.Q8_0"), ("search_query: ", "search_document: "));
        assert_eq!(embed_prefixes("something-else"), ("", ""));
    }

    #[test]
    fn the_layouts_differ_only_where_they_should() {
        // The notes found for each turn differ, so "today" changes the
        // system prompt every turn while "stable" never does.
        assert_ne!(rag_block(0), rag_block(1));
        assert!(NOTES.len() >= 8);
    }

    #[test]
    fn the_retrieval_set_loads() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/retrieval/notes.json");
        let r = load_retrieval(&path).expect("notes.json");
        assert!(r.notes.len() >= 20 && r.queries.len() >= 15);
    }
}

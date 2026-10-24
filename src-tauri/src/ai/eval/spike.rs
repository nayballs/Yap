//! `--raw <file.json>`: send hand-written chat-completions requests, logged
//! like every other call (`transcript.jsonl`), and summarise what came back
//! in `spike.md` — for trying a provider feature before Yap uses it (Groq's
//! built-in `browser_search`, say).
//!
//! The file: `{ "requests": [ { "name": "…", "provider": "groq", "body": {…},
//! "yap_tools": true } ] }`. `yap_tools` adds Chat's own function tools
//! (`tools::definitions` + the app tools) to `body.tools`, after any the body
//! has. The endpoint is the provider's (`ai_model::standard_base_url`, or
//! `base_url` on the request).

use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// Send each request in `file`, in turn. Returns one summary per request.
pub async fn run(file: &std::path::Path, keys: &std::collections::HashMap<String, String>, only: &[String]) -> Result<Vec<Value>, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("can't read {}: {e}", file.display()))?;
    let spec: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    let only: Vec<String> = only.iter().map(|o| o.to_lowercase()).collect();
    let requests: Vec<Value> = spec["requests"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|r| only.is_empty() || only.iter().any(|o| r["name"].as_str().unwrap_or("").to_lowercase().contains(o)))
        .collect();
    let mut out = Vec::new();
    for (i, r) in requests.iter().enumerate() {
        let name = r["name"].as_str().unwrap_or("request").to_string();
        let provider = r["provider"].as_str().unwrap_or("groq").to_string();
        let base = r["base_url"]
            .as_str()
            .map(str::to_string)
            .or_else(|| crate::ai::ai_model::standard_base_url(&provider).map(str::to_string))
            .ok_or_else(|| format!("{name}: no base_url for provider {provider}"))?;
        let endpoint = format!("{}/chat/completions", base.trim_end_matches('/'));
        let key = keys.get(&provider).cloned().unwrap_or_default();
        let mut body = r["body"].clone();
        if r["yap_system"] == json!(true) {
            // Chat's system prompt as `chat_turn` builds it for a model with
            // tools (persona + tool instructions), before the messages.
            let system = format!(
                "{}\n\nYou have access to tools. {} {}",
                super::runner::DEFAULT_PERSONA,
                crate::chat::tools::TOOL_INSTRUCTIONS.join(" "),
                crate::chat::tools::APP_TOOL_INSTRUCTIONS.join(" ")
            );
            let mut messages = vec![json!({ "role": "system", "content": system })];
            messages.extend(body["messages"].as_array().cloned().unwrap_or_default());
            body["messages"] = Value::Array(messages);
        }
        if r["yap_tools"] == json!(true) {
            let mut tools = body["tools"].as_array().cloned().unwrap_or_default();
            tools.extend(crate::chat::tools::definitions().as_array().cloned().unwrap_or_default());
            tools.extend(crate::chat::tools::app_definitions().as_array().cloned().unwrap_or_default());
            body["tools"] = Value::Array(tools);
        }
        super::record::set_context(json!({ "spike": name, "request": i + 1 }));
        super::record::begin_turn();
        println!("  [{}/{}] {name} …", i + 1, requests.len());
        let call = crate::ai::eval::Call::start(&provider, &endpoint, &body).await;
        let started = Instant::now(); // after the pacing
        call.send(&body);
        let client = reqwest::Client::builder().timeout(Duration::from_secs(120)).build().map_err(|e| e.to_string())?;
        let mut req = client.post(&endpoint).json(&body);
        if !key.is_empty() {
            req = req.bearer_auth(&key);
        }
        let summary = match req.send().await {
            Err(e) => {
                let e = call.failed(format!("request failed: {e}"));
                json!({ "name": name, "error": e })
            }
            Ok(resp) => {
                let status = resp.status().as_u16();
                let limits = crate::ai::eval::limit_headers(resp.headers());
                let text = resp.text().await.unwrap_or_default();
                let value: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!(text));
                if (200..300).contains(&status) {
                    call.answered(&limits, &value);
                } else {
                    call.refused(status, &limits, &text);
                }
                summarise(&name, status, started.elapsed(), &limits, &body, &value)
            }
        };
        let stats = super::record::end_turn();
        let mut summary = summary;
        summary["pacing_wait_ms"] = json!(stats.pacing_wait_ms);
        super::record::write("spike_summary", summary.clone());
        out.push(summary);
    }
    Ok(out)
}

/// The shape of one answer: status, time, tokens, the text, any tool calls,
/// and what a server-side tool ran (`executed_tools`: its queries and the
/// pages' titles and URLs), without the bulk.
fn summarise(name: &str, status: u16, took: Duration, limits: &Value, body: &Value, v: &Value) -> Value {
    let msg = &v["choices"][0]["message"];
    let content = msg["content"].as_str().unwrap_or("");
    let executed: Vec<Value> = msg["executed_tools"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| {
            let results = t["search_results"]["results"].as_array().cloned().unwrap_or_default();
            let pages = t["browser_results"].as_array().cloned().unwrap_or_default();
            let link = |r: &Value| json!({ "title": r["title"], "url": r["url"], "content_chars": r["content"].as_str().map_or(0, |c| c.chars().count()), "score": r["score"] });
            json!({
                "type": t["type"],
                "index": t["index"],
                "arguments": t["arguments"],
                "output_chars": t["output"].as_str().map_or(0, |o| o.chars().count()),
                "search_results": results.iter().map(link).collect::<Vec<_>>(),
                "browser_results": pages.iter().map(link).collect::<Vec<_>>(),
                "keys": t.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()),
            })
        })
        .collect();
    let tool_calls: Vec<Value> = msg["tool_calls"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| json!({ "type": c["type"], "name": c["function"]["name"], "arguments": c["function"]["arguments"] }))
        .collect();
    let markers = regex::Regex::new(r"【[^】]*】").map(|re| re.find_iter(content).count()).unwrap_or(0);
    let tools_sent: Vec<String> = body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| match t["function"]["name"].as_str() {
            Some(n) => format!("function:{n}"),
            None => t["type"].as_str().unwrap_or("?").to_string(),
        })
        .collect();
    json!({
        "name": name,
        "status": status,
        "ms": took.as_millis() as u64,
        "model": v["model"],
        "tools_sent": tools_sent,
        "finish_reason": v["choices"][0]["finish_reason"],
        "usage": v["usage"],
        "limits": limits,
        "message_keys": msg.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()),
        "content": content.chars().take(1200).collect::<String>(),
        "content_chars": content.chars().count(),
        "citation_markers": markers,
        "reasoning_chars": msg["reasoning"].as_str().map_or(0, |r| r.chars().count()),
        "tool_calls": tool_calls,
        "executed_tools": executed,
        "error": if (200..300).contains(&status) { Value::Null } else { v.clone() },
    })
}

/// `spike.md`: each request's answer, readable.
pub fn markdown(results: &[Value]) -> String {
    let mut md = String::from("# Raw requests\n\n");
    for r in results {
        md.push_str(&format!("## {}\n\n", r["name"].as_str().unwrap_or("")));
        if let Some(e) = r["error"].as_str() {
            md.push_str(&format!("Request failed: {e}\n\n"));
            continue;
        }
        md.push_str(&format!(
            "- HTTP {} in {} ms (model `{}`), finish_reason `{}`\n- tools sent: {}\n- usage: `{}`\n- rate-limit headers: `{}`\n- message fields: {}\n",
            r["status"],
            r["ms"],
            r["model"].as_str().unwrap_or(""),
            r["finish_reason"].as_str().unwrap_or(""),
            r["tools_sent"],
            r["usage"],
            r["limits"],
            r["message_keys"],
        ));
        if !r["error"].is_null() {
            md.push_str(&format!("- error body: `{}`\n", r["error"]));
        }
        md.push_str(&format!(
            "- content: {} chars, {} citation markers (【…】), reasoning {} chars\n",
            r["content_chars"], r["citation_markers"], r["reasoning_chars"]
        ));
        for c in r["tool_calls"].as_array().into_iter().flatten() {
            md.push_str(&format!("- tool call: `{}` {}\n", c["name"].as_str().unwrap_or(""), c["arguments"]));
        }
        for t in r["executed_tools"].as_array().into_iter().flatten() {
            md.push_str(&format!(
                "- executed tool `{}` args {} (output {} chars, fields {})\n",
                t["type"].as_str().unwrap_or(""),
                t["arguments"],
                t["output_chars"],
                t["keys"]
            ));
            for l in t["search_results"].as_array().into_iter().flatten().chain(t["browser_results"].as_array().into_iter().flatten()) {
                md.push_str(&format!(
                    "  - {} <{}> ({} chars)\n",
                    l["title"].as_str().unwrap_or(""),
                    l["url"].as_str().unwrap_or(""),
                    l["content_chars"]
                ));
            }
        }
        let content = r["content"].as_str().unwrap_or("");
        if !content.is_empty() {
            md.push_str("\n```text\n");
            md.push_str(content);
            md.push_str("\n```\n");
        }
        md.push('\n');
    }
    md
}

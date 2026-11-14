//! **A Code chat's dev servers, for its Claude** (14 Nov 2026; the
//! maintainer: the Claude Code app's Dev servers in its browser pane — "Can
//! we add this to Yap, please? And an ability to do this"): the workspace's
//! `.claude/launch.json` servers (`agents/servers.rs`), as Claude Code's
//! desktop app's own preview tools offer them —
//!
//! - **`preview_start {name}`**: start it (running already: that one) and
//!   open its page in a new tab beside the chat;
//! - **`preview_list`**: the servers in `launch.json`, and those running;
//! - **`preview_stop {serverId}`**;
//! - **`preview_logs {serverId, level?, lines?, search?}`**: its output.
//!
//! **Whose**: a Code chat's own session (its run's folder is the workspace),
//! with the pages on (`claude_live/eyes.rs`); never Chat's own tool loop.
//! Starting one asks nobody, as the Claude Code app's own local preview
//! doesn't (its public docs: external sites ask, local servers don't): the
//! command is the workspace's own, in its own file.

use std::path::Path;

use serde_json::{json, Value};

use super::{failed, tool};
use crate::agents::servers;

pub const START: &str = "preview_start";
pub const LIST: &str = "preview_list";
pub const STOP: &str = "preview_stop";
pub const LOGS: &str = "preview_logs";
/// The dev-server tools, in the order they're listed.
pub const NAMES: &[&str] = &[START, LIST, STOP, LOGS];

/// What `preview_logs` gives at most, and by default.
const LINES_MAX: usize = 500;
const LINES: usize = 50;

/// **Their definitions** (the tool loop's shape).
pub fn definitions() -> Value {
    let s = || json!({ "type": "string" });
    json!([
        tool(
            START,
            "Start one of this workspace's dev servers by name, from .claude/launch.json, and open it in a new tab beside the chat (running already: reused). No launch.json yet: write it first — {\"version\":\"0.0.1\",\"configurations\":[{\"name\":\"<name>\",\"runtimeExecutable\":\"<command, e.g. npm>\",\"runtimeArgs\":[\"run\",\"dev\"],\"port\":<port>}]}. Use this, never Bash, to run a dev server.",
            json!({ "name": s() }),
            &["name"]
        ),
        tool(LIST, "This workspace's dev servers: those in .claude/launch.json, and those running (serverId, port, state).", json!({}), &[]),
        tool(STOP, "Stop a dev server: its serverId (from preview_start or preview_list), or its name.", json!({ "serverId": s(), "name": s() }), &[]),
        tool(
            LOGS,
            "A dev server's output, newest last: build errors, what it logged. level \"error\": errors only; search: lines with these words.",
            json!({ "serverId": s(), "name": s(), "level": { "type": "string", "enum": ["all", "error"] }, "lines": { "type": "integer" }, "search": s() }),
            &[]
        ),
    ])
}

/// What Claude is told of them, with them.
pub const INSTRUCTIONS: &str = "preview_start runs this workspace's dev server from .claude/launch.json (write it first if it's missing) and opens its page beside the chat, where browser_* tools read, look at and act on it: after changing what a running server shows, look at it there to check it works (preview_logs for its errors).";

/// **One call** of a dev-server tool, in `workspace` (`None`: no workspace,
/// refused). `preview_start` opens its page through the browser's own
/// `browser_tabs` (`ctx`: the turn's).
pub async fn execute(turn: &mut crate::web::Turn, name: &str, args: &Value, ctx: &super::Ctx<'_>, workspace: Option<&Path>) -> Value {
    let Some(ws) = workspace else {
        return failed("Dev servers belong to a Code chat's workspace, and this chat has none.".into(), "No dev servers here".into());
    };
    match name {
        START => {
            let want = args["name"].as_str().map(str::trim).unwrap_or("");
            if want.is_empty() {
                let names = servers::read(ws).map(|l| l.configs.iter().map(|c| c.name.clone()).collect::<Vec<_>>()).unwrap_or_default();
                return failed(
                    if names.is_empty() { "name is required, and .claude/launch.json has no servers yet: add one first.".to_string() } else { format!("name is required: one of {}.", names.join(", ")) },
                    "Couldn't start a dev server: no name".into(),
                );
            }
            let already = servers::list_in(ws).into_iter().any(|s| s.name.eq_ignore_ascii_case(want) && matches!(s.state.as_str(), "starting" | "ready"));
            let info = match servers::start(ws, want).await {
                Ok(i) => i,
                Err(e) => return failed(e.clone(), format!("Couldn't start ‘{}’", clip(want, 40))),
            };
            // Its page beside the chat, in a tab of its own.
            let opened = super::execute(turn, super::TABS, &json!({ "action": "new", "url": info.url }), ctx).await;
            let mut data = json!({ "serverId": info.id, "name": info.name, "port": info.port, "url": info.url, "state": info.state });
            if let Some(from) = info.moved_from {
                data["movedFrom"] = json!(from);
            }
            // Its tab's number, as the browser tools take it (Yap Lab, 14 Nov 2026: it said null).
            let tabs = super::backend::tabs().await;
            let tab = tabs.iter().rev().find(|t| t.url == info.url || t.url.trim_end_matches('/') == info.url.trim_end_matches('/')).or_else(|| tabs.iter().find(|t| t.front)).map(|t| json!(t.tab)).unwrap_or(Value::Null);
            let note = if opened.result["success"] == true {
                data["tab"] = tab.clone();
                format!("Its page is open beside the chat (tab {tab}): browser_* tools read, look at and act on it. preview_logs reads its output; it runs until preview_stop or Yap quits.")
            } else {
                format!("It runs, but its page didn't open beside the chat: {}", opened.result["error"].as_str().or_else(|| opened.result["note"].as_str()).unwrap_or("try browser_tabs with its url"))
            };
            let moved = info.moved_from.map(|f| format!(" (port {f} was taken)")).unwrap_or_default();
            let display = if already { format!("Opened ‘{}’ beside the chat (it was running)", info.name) } else { format!("Started ‘{}’ on port {}{moved} beside the chat", info.name, info.port) };
            json!({ "success": true, "data": data, "note": note, "displayText": display })
        }
        LIST => {
            let launch = match servers::read(ws) {
                Ok(l) => l,
                Err(e) => return failed(e, "Couldn't read the dev servers".into()),
            };
            let running = servers::list_in(ws);
            let configs: Vec<Value> = launch
                .configs
                .iter()
                .map(|c| {
                    let s = running.iter().rev().find(|s| s.name == c.name);
                    let mut x = json!({ "name": c.name, "port": c.port, "command": c.line() });
                    if let Some(s) = s {
                        x["serverId"] = json!(s.id);
                        x["state"] = json!(s.state);
                        x["url"] = json!(s.url);
                        if s.port != c.port {
                            x["port"] = json!(s.port);
                        }
                        if let Some(why) = &s.why {
                            x["why"] = json!(why);
                        }
                    }
                    x
                })
                .collect();
            let mut data = json!({ "servers": configs, "launchJson": launch.exists });
            if !launch.skipped.is_empty() {
                data["skipped"] = json!(launch.skipped);
            }
            let note = if !launch.exists { "No .claude/launch.json yet: write one, then preview_start it." } else { "preview_start starts one by name." };
            json!({ "success": true, "data": data, "note": note, "displayText": format!("Listed the dev servers ({})", configs.len()) })
        }
        STOP => {
            let Some(info) = which(ws, args) else {
                return failed("There's no such dev server in this workspace: preview_list lists them (serverId, name).".into(), "Couldn't stop a dev server: not found".into());
            };
            match servers::stop(&info.id) {
                Ok(i) => json!({ "success": true, "data": { "serverId": i.id, "state": i.state }, "displayText": format!("Stopped ‘{}’", i.name) }),
                Err(e) => failed(e.clone(), format!("Couldn't stop ‘{}’", info.name)),
            }
        }
        LOGS => {
            let Some(info) = which(ws, args) else {
                return failed("There's no such dev server in this workspace: preview_list lists them (serverId, name).".into(), "Couldn't read a dev server's output: not found".into());
            };
            let errors = args["level"].as_str().is_some_and(|l| l.eq_ignore_ascii_case("error"));
            let lines = args["lines"].as_u64().map_or(LINES, |n| (n as usize).clamp(1, LINES_MAX));
            let search = args["search"].as_str();
            let out = servers::logs(&info.id, errors, lines, search).unwrap_or_default();
            tracing::info!(lines = out.len(), errors, "Chat: a dev server's output read");
            json!({
                "success": true,
                "data": { "serverId": info.id, "state": info.state, "lines": out.join("\n"), "count": out.len() },
                "note": "Its own output (the newest last).",
                "displayText": format!("Read the output of ‘{}’", info.name),
            })
        }
        _ => failed(format!("No tool {name}."), "Didn't use the dev servers".into()),
    }
}

/// The server a call names in `ws`: by its `serverId`, else its `name` (the
/// newest of that name).
fn which(ws: &Path, args: &Value) -> Option<servers::Info> {
    let all = servers::list_in(ws);
    let id = args["serverId"].as_str().map(str::trim).filter(|s| !s.is_empty());
    let name = args["name"].as_str().map(str::trim).filter(|s| !s.is_empty());
    match (id, name) {
        (Some(id), _) => all.into_iter().find(|s| s.id == id),
        (None, Some(n)) => all.into_iter().rev().find(|s| s.name.eq_ignore_ascii_case(n)),
        _ => None,
    }
}

fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

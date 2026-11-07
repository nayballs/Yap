//! `yap.exe fake-claude` end to end: the real binary started the way Yap
//! starts Claude Code (stream-json both ways, the prompt on stdin), replaying
//! an example script, answering `--version` and `auth status` as Claude Code
//! does. (Debug builds only: a release `yap.exe` has no fake.)

#![cfg(debug_assertions)]

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::channel;
use std::time::Duration;

use serde_json::{json, Value};

fn script(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/ai/claude_code/fixtures/fake").join(name)
}

fn yap() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_yap"));
    cmd.env_remove("YAP_E2E").stderr(Stdio::null());
    cmd
}

#[test]
fn it_answers_version_and_sign_in_status_like_claude_code() {
    let out = yap().args(["fake-claude", "unused.jsonl", "--version"]).output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "2.1.296 (Claude Code)");
    let out = yap().args(["fake-claude", "unused.jsonl", "auth", "status", "--json"]).env("YAP_FAKE_CLAUDE_LOGGED_IN", "0").output().unwrap();
    assert_eq!(out.status.code(), Some(1), "signed out: exit 1, as Claude Code");
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["loggedIn"], false);
    assert_eq!(v["authMethod"], "none");
}

#[test]
fn it_replays_a_conversation_over_stream_json() {
    let mut child = yap()
        .arg("fake-claude")
        .arg(script("chat-two-turns.jsonl"))
        .args(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose"])
        .args(["--session-id", "11111111-2222-4333-8444-555555555555"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("yap.exe fake-claude starts");
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut stdin = child.stdin.take().unwrap();
    let ask = |stdin: &mut std::process::ChildStdin, text: &str| {
        let msg = json!({"type":"user","message":{"role":"user","content":[{"type":"text","text":text}]}});
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
    };
    let next_result = || loop {
        let line = rx.recv_timeout(Duration::from_secs(20)).expect("a line in time");
        let v: Value = serde_json::from_str(&line).unwrap_or_else(|_| panic!("not JSON: {line}"));
        assert_eq!(v["session_id"], "11111111-2222-4333-8444-555555555555", "{line}");
        if v["type"] == "result" {
            return v;
        }
    };
    ask(&mut stdin, "Is \"Imogen\" free at 3?");
    assert_eq!(next_result()["result"], "You asked: Is \"Imogen\" free at 3?");
    // The second turn sleeps: interrupt it.
    ask(&mut stdin, "Think hard");
    std::thread::sleep(Duration::from_millis(300));
    writeln!(stdin, "{}", json!({"type":"control_request","request_id":"req_t1","request":{"subtype":"interrupt"}})).unwrap();
    stdin.flush().unwrap();
    let receipt: Value = loop {
        let v: Value = serde_json::from_str(&rx.recv_timeout(Duration::from_secs(20)).unwrap()).unwrap();
        if v["type"] == "control_response" {
            break v;
        }
    };
    assert_eq!(receipt["response"]["request_id"], "req_t1");
    let aborted: Value = serde_json::from_str(&rx.recv_timeout(Duration::from_secs(20)).unwrap()).unwrap();
    assert_eq!(aborted["terminal_reason"], "aborted_streaming");
    // stdin closed: it exits.
    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success());
}

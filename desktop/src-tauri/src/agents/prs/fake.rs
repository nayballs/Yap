//! `yap.exe fake-gh <script.json> [gh's arguments…]` (debug builds): a
//! stand-in for GitHub's `gh`, for tests (`e2e_gh_script`, or `YAP_GH_EXE`
//! + `YAP_GH_ARGS_PREFIX`). It never reaches GitHub.
//!
//! **The script**, one JSON object: `stdout` (a string printed as it is, or
//! any other JSON printed as JSON: `gh pr list --json`'s array), `stderr`,
//! `code` (its exit code, 0 by default), `sleep_ms` (a slow `gh`), and
//! `dump` (a file it appends `{"args":[…],"cwd":…,"prompts":…}` to: what it
//! was started with, `prompts` whether `GH_PROMPT_DISABLED` was missing).

use std::io::Write;

use serde_json::{json, Value};

/// `yap.exe fake-gh …`'s entry: `args` are what follows `fake-gh`.
pub fn main(args: Vec<String>) -> i32 {
    let Some((script_path, gh_args)) = args.split_first() else {
        eprintln!("usage: yap.exe fake-gh <script.json> [gh's arguments…]");
        return 2;
    };
    let script: Value = match std::fs::read_to_string(script_path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("fake-gh: can't read the script: {e}");
            return 2;
        }
    };
    if let Some(file) = script.get("dump").and_then(Value::as_str) {
        let line = json!({
            "args": gh_args,
            "cwd": std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default(),
            "prompts": std::env::var("GH_PROMPT_DISABLED").map_or(true, |v| v != "1"),
        });
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
            let _ = writeln!(f, "{line}");
        }
    }
    if let Some(ms) = script.get("sleep_ms").and_then(Value::as_u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms.min(60_000)));
    }
    match script.get("stdout") {
        Some(Value::String(s)) => print!("{s}"),
        Some(Value::Null) | None => {}
        Some(v) => println!("{v}"),
    }
    if let Some(s) = script.get("stderr").and_then(Value::as_str) {
        eprint!("{s}");
    }
    script.get("code").and_then(Value::as_i64).unwrap_or(0) as i32
}

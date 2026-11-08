//! **The Claude Code engine**: Yap drives the person's own installed Claude
//! Code CLI as a child process, on their own sign-in (their Claude plan) or
//! their own API key. Chat on your Claude plan and Yap Code build on it.
//!
//! Yap never reads or stores Claude credentials, never signs anyone in
//! itself (sign-in is Claude Code's own `claude auth login`, in a terminal
//! the person owns) and never modifies the binary: Anthropic's terms for
//! running Claude Code in a product (code.claude.com/docs/en/
//! legal-and-compliance, read 7 Nov 2026).
//!
//! - [`cli`]: finding `claude` (the native install, then `PATH`), its
//!   version (at least 2.1.259), whether it's signed in (`claude auth status
//!   --json`), opening the sign-in terminal.
//! - [`session`]: one `claude -p` process in stream-json both ways
//!   ([`session::Spec`] → its flags and environment, the prompt over stdin,
//!   interrupt, end, kill; `claude_plan/oneshot.rs` asks one question on
//!   it).
//! - [`events`]: its output read tolerantly ([`events::Event`]), and what
//!   went wrong derived ([`events::Problem`]: not signed in, at the plan's
//!   limit, paying another way).
//! - [`usage`]: where the plan's limits stand (the experimental
//!   `get_usage` control request on a session, or a one-off probe), read
//!   tolerantly into rows, and moved live by a turn's rate-limit events.
//! - [`transcripts`]: where Claude Code keeps the sessions it ran (its own
//!   config folder, `projects/<the folder, encoded>/<session>.jsonl`), so
//!   Yap can delete a plan chat's with the chat.
//! - `fake` (debug builds): `yap.exe fake-claude <script.jsonl>`, a
//!   stand-in that replays a script, for tests (`YAP_CLAUDE_EXE` +
//!   `YAP_CLAUDE_ARGS_PREFIX`).
//!
//! At start ([`init`]) the run folders a crashed Yap left in
//! `%TEMP%\yap-claude\` are swept ([`session::sweep_run_folders`]).
//!
//! Recordings per Claude Code version in `claude_code/fixtures/<version>/`
//! (`real-*` recorded signed out with an empty config folder, `synthetic-*`
//! built from the documented shapes); the fake's examples in
//! `fixtures/fake/`.

pub mod cli;
pub mod events;
#[cfg(debug_assertions)]
pub mod fake;
pub mod session;
pub mod transcripts;
pub mod usage;

/// At start (lib.rs setup), in the background: the run folders an earlier
/// Yap left behind (a crash) removed, never a running Yap's.
pub fn init() {
    let _ = std::thread::Builder::new().name("claude-code-sweep".into()).spawn(|| {
        session::sweep_run_folders();
    });
}

/// Claude Code on this PC (Settings' plan card): installed, its version,
/// too old, signed in. Cached 30 s; `refresh` probes again.
#[tauri::command]
pub async fn claude_code_status(refresh: Option<bool>) -> cli::Status {
    let refresh = refresh.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || if refresh { cli::refresh() } else { cli::status() })
        .await
        .unwrap_or_else(|_| cli::Status {
            min_version: cli::MIN_VERSION.to_string(),
            error: Some("Couldn't check Claude Code.".into()),
            ..Default::default()
        })
}

/// "Sign in": Windows Terminal (or PowerShell) on `claude auth login`.
#[tauri::command]
pub async fn claude_code_sign_in() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(cli::open_sign_in)
        .await
        .unwrap_or_else(|_| Err("Couldn't open the sign-in.".into()))
}

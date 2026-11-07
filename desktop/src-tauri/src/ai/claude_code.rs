//! **The Claude Code engine**: Yap drives the person's own installed Claude
//! Code CLI as a child process, on their own sign-in (their Claude plan) or
//! their own API key. Chat on your Claude plan and Jobs build on it.
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
//!   interrupt, end, kill; [`session::oneshot`] for one question).
//! - [`events`]: its output read tolerantly ([`events::Event`]), and what
//!   went wrong derived ([`events::Problem`]: not signed in, at the plan's
//!   limit, paying another way).
//! - `fake` (debug builds): `yap.exe fake-claude <script.jsonl>`, a
//!   stand-in that replays a script, for tests (`YAP_CLAUDE_EXE` +
//!   `YAP_CLAUDE_ARGS_PREFIX`).
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

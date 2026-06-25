//! Daily Groq usage tracker for the AI-cleanup layer.
//!
//! Every AI-cleanup call (`llm::cleanup`) accumulates the response's
//! `usage.total_tokens` and the `x-ratelimit-*-requests` headers here. The
//! totals are stamped with a UTC day-number and auto-reset at midnight UTC, so
//! the Settings "Usage today" meter reflects only the current UTC day.
//!
//! Recording is **best-effort**: any error is logged and swallowed so a cleanup
//! is never failed or slowed by usage bookkeeping. After each record we emit a
//! `groq-usage` event with the current snapshot so the Settings panel can update
//! live while the user dictates.
//!
//! Notes on what Groq actually reports:
//! - `usage.total_tokens` is per-call; we sum it. This is **Blip's own** token
//!   use only — we can't see other apps sharing the key.
//! - `x-ratelimit-limit-requests` / `-remaining-requests` are the *daily*
//!   request cap and the requests left today, so `requests = limit - remaining`
//!   is exact. (The header *token* limits are per-minute, so we ignore them for
//!   a daily meter.)
//! - The daily *token* cap is **not** returned by the API, so it's a constant
//!   below, surfaced to the UI as an estimate.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

/// Free-tier daily token cap (llama-3.1-8b-instant default). Not API-reported —
/// shown to the user as an estimate.
pub const GROQ_FREE_TOKEN_CAP: u64 = 500_000;

/// Fallback daily request cap used until Groq's header reports the real one.
const DEFAULT_REQUEST_CAP: u64 = 14_400;

/// Persisted daily usage. `day` is a UTC day-number (unix secs / 86400) so the
/// totals reset when the day rolls over — no date crate needed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct GroqUsage {
    day: u64,
    tokens: u64,
    requests: u64,
    /// Daily request cap reported by Groq's `x-ratelimit-limit-requests`
    /// header (0 until a cloud call has been recorded).
    request_cap: u64,
}

/// In-memory cache of the on-disk usage (loaded lazily on first access).
static STATE: Mutex<Option<GroqUsage>> = Mutex::new(None);

/// App handle for emitting the live `groq-usage` event (set during setup).
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Register the app handle so `record` can emit live updates. Called once at
/// startup; a no-op if called again.
pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

/// Current UTC day-number. Midnight UTC is exactly where unix secs / 86400 ticks
/// over, so comparing this against the stored `day` gives a free daily reset.
fn today() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0)
}

fn usage_path() -> PathBuf {
    crate::config::data_dir().join("groq_usage.json")
}

fn load_from_disk() -> GroqUsage {
    match std::fs::read_to_string(usage_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => GroqUsage::default(),
    }
}

fn save_to_disk(u: &GroqUsage) {
    match serde_json::to_string_pretty(u) {
        Ok(json) => {
            let _ = std::fs::create_dir_all(crate::config::data_dir());
            if let Err(e) = std::fs::write(usage_path(), json) {
                tracing::warn!("Failed to persist Groq usage: {}", e);
            }
        }
        Err(e) => tracing::warn!("Failed to serialize Groq usage: {}", e),
    }
}

/// Reset the counters in place if the stored day isn't today.
fn roll_over(u: &mut GroqUsage) {
    let today = today();
    if u.day != today {
        u.day = today;
        u.tokens = 0;
        u.requests = 0;
        // Keep the last-known request cap across the reset so the bar has a
        // denominator before the first call of the new day comes back.
    }
}

/// JSON snapshot for the command/event: camelCase, with the constant token cap
/// and a sensible request-cap fallback baked in.
fn to_json(u: &GroqUsage) -> Value {
    json!({
        "day": u.day,
        "tokens": u.tokens,
        "tokenCap": GROQ_FREE_TOKEN_CAP,
        "requests": u.requests,
        "requestCap": if u.request_cap > 0 { u.request_cap } else { DEFAULT_REQUEST_CAP },
    })
}

/// Read the current usage (after a same-day check so a new day reads as 0).
pub fn snapshot() -> Value {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let usage = guard.get_or_insert_with(load_from_disk);
    roll_over(usage);
    to_json(usage)
}

/// Record one AI-cleanup call's usage. Best-effort: errors are logged, never
/// propagated. `remaining_requests` / `limit_requests` are Groq's exact daily
/// request headers when present.
pub fn record(total_tokens: u64, remaining_requests: Option<u64>, limit_requests: Option<u64>) {
    let snap = {
        let mut guard = match STATE.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let usage = guard.get_or_insert_with(load_from_disk);
        roll_over(usage);

        usage.tokens = usage.tokens.saturating_add(total_tokens);

        // Prefer Groq's exact daily request math (limit - remaining) when both
        // headers are present; otherwise just count this call.
        match (limit_requests, remaining_requests) {
            (Some(limit), Some(remaining)) => {
                usage.request_cap = limit;
                usage.requests = limit.saturating_sub(remaining);
            }
            _ => {
                usage.requests = usage.requests.saturating_add(1);
            }
        }

        save_to_disk(usage);
        to_json(usage)
    };

    // Emit outside the lock so a slow listener can't hold up the next cleanup.
    if let Some(app) = APP.get() {
        let _ = app.emit("groq-usage", snap);
    }
}

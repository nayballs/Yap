//! Groq's "Usage today" meter (Settings → Language Models), kept in
//! `groq_usage.json` exactly as before the AI usage tracker (`usage.rs`,
//! `ai_usage.json`) came: an older Yap sharing the data folder still reads
//! and writes this file, so its shape never changes.
//!
//! Every AI call (`usage::record_response`) adds the response's total tokens
//! and the `x-ratelimit-*-requests` headers here. The
//! totals are stamped with a UTC day-number and auto-reset at midnight UTC, so
//! the Settings "Usage today" meter reflects only the current UTC day.
//!
//! Recording is **best-effort**: any error is logged and swallowed so a cleanup
//! is never failed or slowed by usage bookkeeping. After each record we emit a
//! `groq-usage` event with the current snapshot so the Settings panel can update
//! live while the user dictates.
//!
//! Notes on what Groq actually reports:
//! - `usage.total_tokens` is per-call; we sum it. This is **Yap's own** token
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

/// Free-tier daily token cap (Groq free plan for the default openai/gpt-oss-20b,
/// same for 120b: 200K tokens/day). Not API-reported — shown to the user as an
/// estimate.
pub const GROQ_FREE_TOKEN_CAP: u64 = 200_000;

/// Fallback daily request cap used until Groq's header reports the real one
/// (free plan: 1K requests/day for the gpt-oss models).
const DEFAULT_REQUEST_CAP: u64 = 1_000;

/// Per-provider daily counts (tokens summed from responses, calls counted
/// locally). Keyed by the provider id ("ondevice", "groq", "openai", …) so the
/// Settings panel can show what THIS provider did, not a global mash-up.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Bucket {
    tokens: u64,
    requests: u64,
}

/// Persisted daily usage. `day` is a UTC day-number (unix secs / 86400) so the
/// totals reset when the day rolls over — no date crate needed.
/// The top-level `tokens`/`requests` are **Groq-only** (they feed the capped
/// free-tier meter); `providers` holds per-provider buckets for everyone.
/// Old files without `providers` load fine — their totals stay attributed to
/// Groq, which is where they historically came from.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct GroqUsage {
    day: u64,
    tokens: u64,
    requests: u64,
    /// Daily request cap reported by Groq's `x-ratelimit-limit-requests`
    /// header (0 until a cloud call has been recorded).
    request_cap: u64,
    #[serde(default)]
    providers: std::collections::BTreeMap<String, Bucket>,
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

/// groq_usage.json as it loads (`saving::Saver::load`: set aside if it
/// isn't the counts; one Yap couldn't read is never written over until it
/// reads, [`loaded`]). A unit test never touches the real one.
fn load_from_disk() -> GroqUsage {
    if cfg!(test) {
        return GroqUsage::default();
    }
    SAVER.load(&usage_path())
}

/// The counts, read the first time; while the file couldn't be read, read
/// again and, once it reads, kept together with today's counts made
/// meanwhile (the same day: the larger of each; another day: the later).
fn loaded(slot: &mut Option<GroqUsage>) -> &mut GroqUsage {
    let usage = slot.get_or_insert_with(load_from_disk);
    if !cfg!(test) && SAVER.any_unread() {
        if let Some(disk) = SAVER.reread::<GroqUsage>(&usage_path(), crate::shell::saving::json) {
            let cap = usage.request_cap.max(disk.request_cap);
            match disk.day.cmp(&usage.day) {
                std::cmp::Ordering::Equal => crate::shell::saving::merge_json(usage, disk),
                std::cmp::Ordering::Greater => *usage = disk,
                std::cmp::Ordering::Less => {}
            }
            usage.request_cap = cap;
            save_to_disk(usage);
        }
    }
    usage
}

/// groq_usage.json's saves (`saving.rs`): a failed one keeps the counts in
/// memory, is tried again with the next AI call's save, and is said once.
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::new("usage", "your AI usage counts", save_now);

/// Write the counts again (Try again).
fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if guard.is_some() {
        save_to_disk(loaded(&mut guard));
    }
    drop(guard);
    SAVER.check()
}

fn save_to_disk(u: &GroqUsage) {
    let _ = SAVER.write_json(&usage_path(), u, true);
}

/// Reset the counters in place if the stored day isn't today.
fn roll_over(u: &mut GroqUsage) {
    let today = today();
    if u.day != today {
        u.day = today;
        u.tokens = 0;
        u.requests = 0;
        u.providers.clear();
        // Keep the last-known request cap across the reset so the bar has a
        // denominator before the first call of the new day comes back.
    }
}

/// JSON snapshot for the command/event: camelCase, with the constant token cap
/// and a sensible request-cap fallback baked in.
fn to_json(u: &GroqUsage) -> Value {
    let providers: serde_json::Map<String, Value> = u
        .providers
        .iter()
        .map(|(k, b)| (k.clone(), json!({ "tokens": b.tokens, "requests": b.requests })))
        .collect();
    json!({
        "day": u.day,
        "tokens": u.tokens,
        "tokenCap": GROQ_FREE_TOKEN_CAP,
        "requests": u.requests,
        "requestCap": if u.request_cap > 0 { u.request_cap } else { DEFAULT_REQUEST_CAP },
        "providers": providers,
    })
}

/// Reset app (`privacy/reset.rs`): today's counts gone (the file too);
/// Groq's daily request cap, learnt from its answers, is kept.
pub fn reset() {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let cap = loaded(&mut guard).request_cap;
    let usage = GroqUsage { day: today(), request_cap: cap, ..GroqUsage::default() };
    if !cfg!(test) {
        match std::fs::remove_file(usage_path()) {
            Ok(()) => SAVER.nothing_to_save(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => SAVER.nothing_to_save(),
            Err(_) => save_to_disk(&usage),
        }
    }
    let snap = to_json(&usage);
    *guard = Some(usage);
    drop(guard);
    if let Some(app) = APP.get() {
        let _ = app.emit("groq-usage", snap);
    }
}

/// Read the current usage (after a same-day check so a new day reads as 0).
pub fn snapshot() -> Value {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let usage = loaded(&mut guard);
    roll_over(usage);
    to_json(usage)
}

/// Record one AI-cleanup call's usage against `provider` (the id actually used
/// for the call, e.g. "ondevice" or "groq"). Best-effort: errors are logged,
/// never propagated. `remaining_requests` / `limit_requests` are Groq's exact
/// daily request headers when present.
pub fn record(
    provider: &str,
    total_tokens: u64,
    remaining_requests: Option<u64>,
    limit_requests: Option<u64>,
) {
    // Unit tests that talk to a local fake model never write groq_usage.json.
    if cfg!(test) {
        return;
    }
    let snap = {
        let mut guard = match STATE.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let usage = loaded(&mut guard);
        roll_over(usage);

        // Per-provider bucket: what THIS provider did today.
        let bucket = usage.providers.entry(provider.to_string()).or_default();
        bucket.tokens = bucket.tokens.saturating_add(total_tokens);
        bucket.requests = bucket.requests.saturating_add(1);

        // The top-level (capped-meter) totals are Groq's alone. Gating on the
        // provider also stops other endpoints' x-ratelimit headers (OpenAI
        // sends them too) from overwriting Groq's request math.
        if provider == "groq" {
            usage.tokens = usage.tokens.saturating_add(total_tokens);
            // Prefer Groq's exact daily request math (limit - remaining) when
            // both headers are present; otherwise just count this call.
            match (limit_requests, remaining_requests) {
                (Some(limit), Some(remaining)) => {
                    usage.request_cap = limit;
                    usage.requests = limit.saturating_sub(remaining);
                }
                _ => {
                    usage.requests = usage.requests.saturating_add(1);
                }
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

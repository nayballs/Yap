//! The run's recorder: `transcript.jsonl` (every request, response, tool call
//! and turn, keys redacted), the per-turn tallies the scoring reads, the
//! settings and tool switch the hooks hand the app, and the pacing that keeps
//! a run under a provider's tokens-per-minute limit.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

/// What one turn cost and did: the model calls (steps), tokens, time, waits,
/// tool calls, and what the clipboard tool was handed.
#[derive(Debug, Default, Clone, Serialize)]
pub struct TurnStats {
    /// Model calls (each counted once, however many attempts it took).
    pub requests: u32,
    /// Attempts beyond the first (a 429 waited out, a temperature retry).
    pub retries: u32,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    /// Prompt tokens the provider served from its cache (when it says).
    pub cached_tokens: u64,
    /// Completion tokens spent reasoning (when the provider says).
    pub reasoning_tokens: u64,
    /// Time spent waiting for model answers.
    pub model_ms: u64,
    /// The slowest single answer, and how many took longer than Chat's own
    /// 20 s timeout (which a run with a longer `--timeout` lets through).
    pub slowest_call_ms: u64,
    pub calls_over_20s: u32,
    /// HTTP 429s, and the time Yap's own retry logic waited them out.
    pub rate_limited: u32,
    pub rate_limit_wait_ms: u64,
    /// Time the run's pacing held calls back (before they went).
    pub pacing_wait_ms: u64,
    /// Calls that failed in the end.
    pub failed_calls: u32,
    /// llama.cpp's own timings, summed over the turn (local models).
    pub prompt_eval_tokens: u64,
    pub prompt_eval_ms: f64,
    pub gen_tokens: u64,
    pub gen_ms: f64,
    pub tool_calls: Vec<ToolCall>,
    /// The text the clipboard tool was asked to copy (recorded, never copied).
    pub clipboard: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCall {
    pub name: String,
    pub args: Value,
    pub success: bool,
    pub display: String,
    pub result: Value,
}

struct Recorder {
    out: Option<std::io::BufWriter<std::fs::File>>,
    secrets: Vec<String>,
    /// Merged into every record: the model, scenario and turn.
    context: serde_json::Map<String, Value>,
    calls: u64,
    turn: TurnStats,
    /// Pacing: tokens used per call over the last minute, and the budget.
    window: VecDeque<(Instant, u64)>,
    tokens_per_minute: Option<u64>,
    /// The provider's own view, from its last answer's headers: when its
    /// token window is full again, how much was left, and its limit.
    server: Option<ServerWindow>,
    paced_ms: u64,
}

#[derive(Clone, Copy)]
struct ServerWindow {
    at: Instant,
    remaining: u64,
    limit: u64,
    reset: Duration,
}

static REC: Mutex<Option<Recorder>> = Mutex::new(None);
static CONFIG: Mutex<Option<crate::config::YapConfig>> = Mutex::new(None);
static TOOLS: Mutex<Option<bool>> = Mutex::new(None);
static TIMEOUT: Mutex<Option<Duration>> = Mutex::new(None);

pub fn set_timeout(t: Option<Duration>) {
    *TIMEOUT.lock().unwrap_or_else(|p| p.into_inner()) = t;
}

pub fn timeout() -> Option<Duration> {
    *TIMEOUT.lock().unwrap_or_else(|p| p.into_inner())
}

fn rec() -> std::sync::MutexGuard<'static, Option<Recorder>> {
    REC.lock().unwrap_or_else(|p| p.into_inner())
}

/// Start recording into `transcript` (`None`: tallies only).
pub fn open(transcript: Option<&std::path::Path>, secrets: Vec<String>, tokens_per_minute: Option<u64>) -> Result<(), String> {
    let out = match transcript {
        Some(p) => Some(std::io::BufWriter::new(
            std::fs::File::create(p).map_err(|e| format!("can't write {}: {e}", p.display()))?,
        )),
        None => None,
    };
    *rec() = Some(Recorder {
        out,
        secrets: secrets.into_iter().filter(|s| s.len() >= 8).collect(),
        context: serde_json::Map::new(),
        calls: 0,
        turn: TurnStats::default(),
        window: VecDeque::new(),
        tokens_per_minute,
        server: None,
        paced_ms: 0,
    });
    Ok(())
}

/// Flush and stop.
pub fn close() {
    if let Some(r) = rec().as_mut() {
        if let Some(out) = r.out.as_mut() {
            let _ = out.flush();
        }
    }
    *rec() = None;
}

/// Add a secret to redact (a key found later).
pub fn add_secret(secret: &str) {
    if secret.len() >= 8 {
        if let Some(r) = rec().as_mut() {
            if !r.secrets.iter().any(|s| s == secret) {
                r.secrets.push(secret.to_string());
            }
        }
    }
}

/// `s` with every known key, and anything shaped like one, replaced.
pub fn redact(s: &str) -> String {
    let secrets = rec().as_ref().map(|r| r.secrets.clone()).unwrap_or_default();
    redact_with(s, &secrets)
}

fn redact_with(s: &str, secrets: &[String]) -> String {
    use std::sync::OnceLock;
    static SHAPES: OnceLock<regex::Regex> = OnceLock::new();
    let mut out = s.to_string();
    for k in secrets {
        if out.contains(k.as_str()) {
            out = out.replace(k.as_str(), "[REDACTED]");
        }
    }
    // Belt and braces: Groq, OpenAI, Anthropic, OpenRouter and Google keys.
    let shapes = SHAPES.get_or_init(|| {
        regex::Regex::new(r"\b(gsk_[A-Za-z0-9]{20,}|sk-(?:ant-|or-|proj-)?[A-Za-z0-9_\-]{20,}|AIza[0-9A-Za-z_\-]{30,})")
            .expect("static regex")
    });
    shapes.replace_all(&out, "[REDACTED]").into_owned()
}

/// Write one record (the context merged in), redacted.
pub fn write(kind: &str, mut record: Value) {
    let mut guard = rec();
    let Some(r) = guard.as_mut() else { return };
    if let Some(obj) = record.as_object_mut() {
        let mut full = serde_json::Map::new();
        full.insert("type".into(), json!(kind));
        full.insert("at".into(), json!(chrono::Local::now().format("%H:%M:%S%.3f").to_string()));
        for (k, v) in &r.context {
            full.insert(k.clone(), v.clone());
        }
        full.append(obj);
        record = Value::Object(full);
    }
    let line = redact_with(&record.to_string(), &r.secrets);
    if let Some(out) = r.out.as_mut() {
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
}

/// The model / scenario / turn every record carries from now on.
pub fn set_context(context: Value) {
    if let (Some(r), Value::Object(map)) = (rec().as_mut(), context) {
        r.context = map;
    }
}

/// Start a turn's tallies afresh.
pub fn begin_turn() {
    if let Some(r) = rec().as_mut() {
        r.turn = TurnStats::default();
    }
}

/// The turn's tallies (and start the next afresh).
pub fn end_turn() -> TurnStats {
    rec().as_mut().map(|r| std::mem::take(&mut r.turn)).unwrap_or_default()
}

/// Total time the pacing held calls back so far.
pub fn paced_ms() -> u64 {
    rec().as_ref().map_or(0, |r| r.paced_ms)
}

pub fn set_tokens_per_minute(tpm: Option<u64>) {
    if let Some(r) = rec().as_mut() {
        r.tokens_per_minute = tpm;
        r.window.clear();
        r.server = None;
    }
}

pub fn set_config(cfg: Option<crate::config::YapConfig>) {
    *CONFIG.lock().unwrap_or_else(|p| p.into_inner()) = cfg;
}

pub fn config() -> Option<crate::config::YapConfig> {
    CONFIG.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

pub fn set_tools_forced(forced: Option<bool>) {
    *TOOLS.lock().unwrap_or_else(|p| p.into_inner()) = forced;
}

pub fn tools_forced() -> Option<bool> {
    *TOOLS.lock().unwrap_or_else(|p| p.into_inner())
}

/// The clipboard tool: recorded, answered as the real one answers.
pub fn intercept_tool(name: &str, args: &Value) -> Option<Value> {
    if name != "copy_to_clipboard" {
        return None;
    }
    let text = args["text"].as_str().unwrap_or("").to_string();
    let preview: String = if text.chars().count() > 100 {
        format!("{}...", text.chars().take(100).collect::<String>())
    } else {
        text.clone()
    };
    if let Some(r) = rec().as_mut() {
        r.turn.clipboard = Some(text.clone());
    }
    write("clipboard", json!({ "text": text }));
    Some(json!({ "success": true, "data": Value::Null, "displayText": format!("Copied to clipboard: \"{preview}\"") }))
}

pub fn tool_called(name: &str, args: &Value, result: &Value) {
    let call = ToolCall {
        name: name.to_string(),
        args: args.clone(),
        success: result["success"] == json!(true),
        display: result["displayText"].as_str().unwrap_or("").to_string(),
        result: result.clone(),
    };
    if let Some(r) = rec().as_mut() {
        r.turn.tool_calls.push(call);
    }
    write("tool_call", json!({ "name": name, "args": args, "result": result }));
}

pub fn limit_headers(headers: &reqwest::header::HeaderMap) -> Value {
    let mut out = serde_json::Map::new();
    for (k, v) in headers {
        let k = k.as_str();
        if k.starts_with("x-ratelimit") || k.starts_with("retry-after") || k == "x-request-id" || k == "x-groq-region" {
            out.insert(k.to_string(), json!(v.to_str().unwrap_or("")));
        }
    }
    Value::Object(out)
}

/// "7.66s", "1m2.5s", "450ms" → a duration (Groq's reset headers).
pub fn parse_reset(s: &str) -> Option<Duration> {
    let mut rest = s.trim();
    let mut total = 0.0;
    let mut any = false;
    while !rest.is_empty() {
        let n = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len());
        let v: f64 = rest[..n].parse().ok()?;
        rest = &rest[n..];
        let (scale, len) = if rest.starts_with("ms") {
            (0.001, 2)
        } else if rest.starts_with('h') {
            (3600.0, 1)
        } else if rest.starts_with('m') {
            (60.0, 1)
        } else if rest.starts_with('s') {
            (1.0, 1)
        } else if rest.is_empty() {
            (1.0, 0)
        } else {
            return None;
        };
        total += v * scale;
        any = true;
        rest = &rest[len..];
    }
    any.then(|| Duration::from_secs_f64(total))
}

/// A rough token count for a request body: ~4.8 characters of JSON a token
/// (measured on Chat's requests to gpt-oss-20b: 10.4k characters → 2.06k
/// prompt tokens), plus what the answer may take.
fn estimate_tokens(body: &Value) -> u64 {
    (body.to_string().len() as f64 / 4.8) as u64 + 400
}

/// How long to hold a call of `estimate` tokens back. Pure.
fn pacing_wait(
    now: Instant,
    window: &VecDeque<(Instant, u64)>,
    budget: Option<u64>,
    server: Option<ServerWindow>,
    estimate: u64,
) -> Duration {
    let minute = Duration::from_secs(60);
    let mut wait = Duration::ZERO;
    // The provider's own count, when it gave one in the last minute, is the
    // truth (it knows what other clients of the key used); else our window.
    let server_fresh = server.is_some_and(|s| s.limit > 0 && now.duration_since(s.at) < minute);
    if let Some(budget) = budget.filter(|_| !server_fresh) {
        let live: Vec<(Instant, u64)> = window.iter().copied().filter(|(t, _)| now.duration_since(*t) < minute).collect();
        let used: u64 = live.iter().map(|(_, n)| n).sum();
        if used + estimate > budget && !live.is_empty() {
            // Wait until enough of the oldest calls have left the minute.
            let need = (used + estimate).saturating_sub(budget);
            let mut freed = 0;
            for (t, n) in &live {
                freed += n;
                if freed >= need || estimate > budget {
                    wait = wait.max((*t + minute).saturating_duration_since(now));
                    if freed >= need && estimate <= budget {
                        break;
                    }
                }
            }
        }
    }
    if let Some(s) = server {
        // The provider's window refills at limit/60 tokens a second.
        let elapsed = now.duration_since(s.at);
        if s.limit > 0 && elapsed < s.reset {
            let refilled = (elapsed.as_secs_f64() * s.limit as f64 / 60.0) as u64;
            let have = (s.remaining + refilled).min(s.limit);
            if have < estimate.min(s.limit) {
                let short = estimate.min(s.limit) - have;
                let secs = short as f64 * 60.0 / s.limit as f64 + 0.5;
                wait = wait.max(Duration::from_secs_f64(secs).min(s.reset.saturating_sub(elapsed) + Duration::from_millis(500)));
            }
        }
    }
    wait
}

/// One model call's log (see `eval::Call`).
pub struct CallLog {
    n: u64,
    attempts: AtomicU32,
    sent: Mutex<Instant>,
}

impl CallLog {
    pub async fn start(provider: &str, endpoint: &str, body: &Value) -> CallLog {
        let estimate = estimate_tokens(body);
        loop {
            let wait = {
                let guard = rec();
                match guard.as_ref() {
                    Some(r) => pacing_wait(Instant::now(), &r.window, r.tokens_per_minute, r.server, estimate),
                    None => Duration::ZERO,
                }
            };
            if wait < Duration::from_millis(50) {
                break;
            }
            let (used, budget) = {
                let guard = rec();
                let r = guard.as_ref();
                let now = Instant::now();
                (
                    r.map_or(0, |r| {
                        r.window.iter().filter(|(t, _)| now.duration_since(*t).as_secs() < 60).map(|(_, n)| n).sum::<u64>()
                    }),
                    r.and_then(|r| r.tokens_per_minute),
                )
            };
            write(
                "pacing",
                json!({ "wait_ms": wait.as_millis() as u64, "estimate": estimate, "used_last_minute": used, "budget": budget }),
            );
            tokio::time::sleep(wait).await;
            if let Some(r) = rec().as_mut() {
                r.paced_ms += wait.as_millis() as u64;
                r.turn.pacing_wait_ms += wait.as_millis() as u64;
            }
        }
        let n = {
            let mut guard = rec();
            match guard.as_mut() {
                Some(r) => {
                    r.calls += 1;
                    r.turn.requests += 1;
                    r.calls
                }
                None => 0,
            }
        };
        write("call", json!({ "call": n, "provider": provider, "endpoint": endpoint, "estimate": estimate }));
        CallLog { n, attempts: AtomicU32::new(0), sent: Mutex::new(Instant::now()) }
    }

    pub fn send(&self, body: &Value) {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
        *self.sent.lock().unwrap_or_else(|p| p.into_inner()) = Instant::now();
        if attempt > 1 {
            if let Some(r) = rec().as_mut() {
                r.turn.retries += 1;
            }
        }
        write("request", json!({ "call": self.n, "attempt": attempt, "body": body }));
    }

    fn ms(&self) -> u64 {
        self.sent.lock().unwrap_or_else(|p| p.into_inner()).elapsed().as_millis() as u64
    }

    pub fn refused(&self, status: u16, limits: &Value, detail: &str) {
        let ms = self.ms();
        if let Some(r) = rec().as_mut() {
            r.turn.model_ms += ms;
            if status == 429 {
                r.turn.rate_limited += 1;
            }
        }
        let body: Value = serde_json::from_str(detail).unwrap_or_else(|_| json!(detail));
        write(
            "refused",
            json!({ "call": self.n, "attempt": self.attempts.load(Ordering::SeqCst), "status": status, "ms": ms, "limits": limits, "body": body }),
        );
    }

    pub fn waiting(&self, wait: Duration) {
        if let Some(r) = rec().as_mut() {
            r.turn.rate_limit_wait_ms += wait.as_millis() as u64;
        }
        write("rate_limit_wait", json!({ "call": self.n, "wait_ms": wait.as_millis() as u64 }));
    }

    pub fn failed(&self, error: &str) {
        if let Some(r) = rec().as_mut() {
            r.turn.failed_calls += 1;
        }
        write("call_failed", json!({ "call": self.n, "error": error }));
    }

    pub fn answered(&self, limits: &Value, body: &Value) {
        let ms = self.ms();
        let usage = &body["usage"];
        let total = usage["total_tokens"].as_u64().unwrap_or(0);
        let cached = usage["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0);
        let now = Instant::now();
        if let Some(r) = rec().as_mut() {
            let t = &mut r.turn;
            t.model_ms += ms;
            t.slowest_call_ms = t.slowest_call_ms.max(ms);
            if ms > 20_000 {
                t.calls_over_20s += 1;
            }
            t.prompt_tokens += usage["prompt_tokens"].as_u64().unwrap_or(0);
            t.completion_tokens += usage["completion_tokens"].as_u64().unwrap_or(0);
            t.total_tokens += total;
            t.cached_tokens += cached;
            t.reasoning_tokens += usage["completion_tokens_details"]["reasoning_tokens"].as_u64().unwrap_or(0);
            let timings = &body["timings"];
            t.prompt_eval_tokens += timings["prompt_n"].as_u64().unwrap_or(0);
            t.prompt_eval_ms += timings["prompt_ms"].as_f64().unwrap_or(0.0);
            t.gen_tokens += timings["predicted_n"].as_u64().unwrap_or(0);
            t.gen_ms += timings["predicted_ms"].as_f64().unwrap_or(0.0);
            r.window.push_back((now, total.max(1)));
            while r.window.len() > 200 {
                r.window.pop_front();
            }
            let h = |k: &str| limits[k].as_str().map(str::to_string);
            if let (Some(rem), Some(reset)) = (
                h("x-ratelimit-remaining-tokens").and_then(|v| v.parse::<u64>().ok()),
                h("x-ratelimit-reset-tokens").and_then(|v| parse_reset(&v)),
            ) {
                let limit = h("x-ratelimit-limit-tokens").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
                r.server = Some(ServerWindow { at: now, remaining: rem, limit, reset });
            }
        }
        write(
            "response",
            json!({ "call": self.n, "attempt": self.attempts.load(Ordering::SeqCst), "ms": ms, "limits": limits, "body": body }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_never_reach_the_log() {
        let s = r#"{"auth":"Bearer gsk_abcdefghijklmnopqrstuvwxyz0123456789","k":"my-own-secret-key"}"#;
        let out = redact_with(s, &["my-own-secret-key".to_string()]);
        assert!(!out.contains("gsk_abc"), "{out}");
        assert!(!out.contains("my-own-secret-key"), "{out}");
        assert_eq!(out.matches("[REDACTED]").count(), 2);
        assert_eq!(redact_with("sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUV", &[]), "[REDACTED]");
        assert_eq!(redact_with("nothing secret here", &[]), "nothing secret here");
    }

    #[test]
    fn reset_headers_parse() {
        let ms = |s: &str| parse_reset(s).map(|d| d.as_millis());
        assert_eq!(ms("7.66s"), Some(7_660));
        assert_eq!(ms("1m2.5s"), Some(62_500));
        assert_eq!(ms("450ms"), Some(450));
        assert_eq!(ms("2"), Some(2_000));
        assert_eq!(ms("soon"), None);
    }

    #[test]
    fn pacing_waits_for_the_minute_to_free_up() {
        let now = Instant::now();
        let mut window = VecDeque::new();
        // Nothing used: no wait.
        assert_eq!(pacing_wait(now, &window, Some(8_000), None, 4_000), Duration::ZERO);
        // 5k used 20 s ago: a 4k call waits ~40 s for it to leave the minute.
        window.push_back((now - Duration::from_secs(20), 5_000));
        let w = pacing_wait(now, &window, Some(8_000), None, 4_000);
        assert!((39..=41).contains(&w.as_secs()), "{w:?}");
        // A call bigger than the budget waits for an empty minute.
        let w = pacing_wait(now, &window, Some(3_000), None, 4_000);
        assert!((39..=41).contains(&w.as_secs()), "{w:?}");
        // No budget: no wait.
        assert_eq!(pacing_wait(now, &window, None, None, 4_000), Duration::ZERO);
        // The provider said 1,000 left of 8,000: 3,000 more come in ~22.5 s.
        let server = ServerWindow { at: now, remaining: 1_000, limit: 8_000, reset: Duration::from_secs(52) };
        let w = pacing_wait(now, &VecDeque::new(), None, Some(server), 4_000);
        assert!((22..=24).contains(&w.as_secs()), "{w:?}");
        // Its count, fresh, wins over ours: 6k left, so a full window of ours
        // doesn't hold a 4k call back.
        let server = ServerWindow { at: now, remaining: 6_000, limit: 8_000, reset: Duration::from_secs(15) };
        assert_eq!(pacing_wait(now, &window, Some(8_000), Some(server), 4_000), Duration::ZERO);
    }
}

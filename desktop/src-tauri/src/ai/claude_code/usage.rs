//! **Where a Claude plan's limits stand** (2026-11-07): Claude Code's
//! experimental `get_usage` control request, read tolerantly into a small
//! [`PlanUsage`] (the plan, its limits as rows, extra usage).
//!
//! - **The request** ([`request`]): `{"subtype":"get_usage",
//!   "skip_behaviors":true}` (`skip_behaviors`: no scan of Claude Code's
//!   local transcripts, as for a usage meter). On a live session
//!   (`Session::get_usage`) or a one-off process ([`probe`]): the run's
//!   flags as the plan's one-shot calls have them (`--tools ""`,
//!   `--strict-mcp-config`, `--setting-sources ""`,
//!   `--no-session-persistence`), nothing sent but the request, stdin
//!   closed once it's answered, ended if it isn't in time. No model call
//!   (checked on 2.1.296 signed in on a Max plan: 1.9 s, no prompt sent).
//! - **The answer** ([`parse`]): a `control_response` whose
//!   `response.response` (or `response`) has `subscription_type`,
//!   `rate_limits_available` (false on an API key, Bedrock or Vertex: no
//!   numbers) and `rate_limits`: `limits[]` (`kind` — `weekly_all`,
//!   `weekly_scoped`, … —, `group`, `percent`, `severity`, `resets_at`,
//!   `scope.model.display_name`, `is_active`), the named windows
//!   (`five_hour`, `seven_day`, `seven_day_opus`, `seven_day_sonnet`,
//!   `model_scoped[]`), `extra_usage` and `spend`. **Experimental**, so:
//!   `limits[]` first, the named windows when it gives nothing (and the
//!   five-hour window as "Current session" when `limits[]` has no session
//!   row); a kind Yap can't place, a field missing or of another type, a
//!   scoped limit at 0 % that isn't active: left out, never an error.
//! - **Live** ([`PlanUsage::apply`]): a turn's `rate_limit_event`s move the
//!   rows they name (`rateLimitType`, `unifiedWindows`) until the next
//!   answer.
//!
//! The answer's other fields (account details, behaviours, code-named
//! limits) are never read. It's Claude Code's own request to Anthropic,
//! never Yap's, so it isn't in Network connections.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedReceiver;

use super::cli::{self, Program};
use super::events::{ControlResponse, Event, RateLimit};
use super::session::{Pay, Session, Spec};

/// The control request.
pub fn request() -> Value {
    json!({"subtype": "get_usage", "skip_behaviors": true})
}

/// How close a limit is, as the answer says it (else from its percent).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    #[default]
    Normal,
    Warning,
    Critical,
}

impl Severity {
    /// The answer's word: "normal", "warning", "critical" (and a few
    /// likely others); `None` for one Yap doesn't know.
    fn parse(word: &str) -> Option<Severity> {
        match word.trim().to_ascii_lowercase().as_str() {
            "normal" | "ok" | "info" | "none" | "low" => Some(Severity::Normal),
            "warning" | "warn" | "approaching" | "elevated" | "medium" | "high" => Some(Severity::Warning),
            "critical" | "error" | "exceeded" | "blocked" | "rejected" | "limited" => Some(Severity::Critical),
            _ => None,
        }
    }

    /// From a percent alone (the named windows say no severity).
    fn of(percent: f64) -> Severity {
        if percent >= 100.0 {
            Severity::Critical
        } else if percent >= 80.0 {
            Severity::Warning
        } else {
            Severity::Normal
        }
    }
}

/// One limit, as a row of "Plan usage limits".
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit {
    /// The answer's own kind (`weekly_all`, `weekly_scoped`), or the named
    /// window's (`five_hour`).
    pub kind: String,
    /// The label's first part: "Current session", "Weekly", "Daily",
    /// "Monthly".
    pub title: String,
    /// Its second: "all models", or a scoped limit's model ("Fable");
    /// none for the current session.
    pub scope: Option<String>,
    /// One model's (or one surface's) limit.
    pub scoped: bool,
    /// 0–100 (more past the limit).
    pub percent: f64,
    pub severity: Severity,
    /// Unix seconds.
    pub resets_at: Option<i64>,
    pub active: bool,
    /// The `rateLimitType` a turn's rate-limit events name it by
    /// (`five_hour`, `seven_day`, `seven_day_opus`…).
    #[serde(skip)]
    pub window: Option<String>,
}

/// Extra usage (paid past the plan's limits), when it's on.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Extra {
    /// Spent this month, in `currency`.
    pub used: f64,
    /// The monthly limit (none set: `None`).
    pub limit: Option<f64>,
    /// ISO 4217 ("GBP").
    pub currency: String,
}

/// **The plan's usage**, what Yap shows of `get_usage`'s answer.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanUsage {
    /// "pro", "max", "team", "enterprise", … as Claude Code says.
    pub subscription: Option<String>,
    /// Session first, then weekly (all models, then each scoped one).
    pub limits: Vec<Limit>,
    pub extra: Option<Extra>,
}

/// What asking came to.
#[derive(Clone, Debug, PartialEq)]
pub enum Asked {
    /// Answered: the numbers, or `None` when there are none to show (an
    /// API key's login, Bedrock, Vertex, a shape Yap can't read).
    Answered(Option<PlanUsage>),
    /// This Claude Code said no (one without `get_usage`).
    Refused,
    /// No answer in time, the process ended first, or it couldn't start.
    NoAnswer,
}

// ---- Reading the answer ----

/// The answer to `get_usage` in a `control_response`.
pub fn from_response(c: &ControlResponse) -> Asked {
    if !c.ok {
        return Asked::Refused;
    }
    let body = [&c.response, &c.whole].into_iter().find(|v| looks_like_usage(v));
    Asked::Answered(body.and_then(parse))
}

fn looks_like_usage(v: &Value) -> bool {
    v.is_object() && ["rate_limits", "rate_limits_available", "subscription_type"].iter().any(|k| v.get(*k).is_some())
}

/// **`get_usage`'s body → what Yap shows**: `response.response` (or the
/// body itself). `None` when there's nothing to show. Pure.
pub fn parse(v: &Value) -> Option<PlanUsage> {
    let body = if looks_like_usage(v) { v } else { v.get("response").filter(|r| looks_like_usage(r))? };
    if body.get("rate_limits_available").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let rl = body.get("rate_limits").filter(|r| r.is_object())?;
    let mut limits: Vec<Limit> = rl.get("limits").and_then(Value::as_array).map(|a| a.iter().filter_map(from_limit).collect()).unwrap_or_default();
    if limits.is_empty() {
        limits = named_windows(rl);
    } else if !limits.iter().any(|l| l.title == SESSION) {
        // The five-hour window, when `limits[]` has no session row.
        limits.extend(window_row(rl, "five_hour"));
    }
    limits.retain(|l| !(l.scoped && l.percent <= 0.0 && !l.active));
    limits.sort_by_key(|l| (period_rank(&l.title), l.scoped));
    let extra = extra(rl);
    if limits.is_empty() && extra.is_none() {
        return None;
    }
    let subscription = body.get("subscription_type").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    Some(PlanUsage { subscription, limits, extra })
}

const SESSION: &str = "Current session";

/// A period's title from a word of the answer's (`weekly`, `five_hour`,
/// `seven_day`, `session`…).
fn period(word: &str) -> Option<&'static str> {
    match word.trim().to_ascii_lowercase().as_str() {
        "session" | "current_session" | "five_hour" | "5h" | "5_hour" => Some(SESSION),
        "weekly" | "week" | "seven_day" | "7d" | "7_day" => Some("Weekly"),
        "daily" | "day" => Some("Daily"),
        "monthly" | "month" => Some("Monthly"),
        _ => None,
    }
}

fn period_rank(title: &str) -> u8 {
    match title {
        SESSION => 0,
        "Daily" => 1,
        "Weekly" => 2,
        "Monthly" => 3,
        _ => 4,
    }
}

/// A row from one of `limits[]`: its kind `<period>_all`, `<period>_scoped`
/// or `<period>`; anything else (or no percent, or a scoped one with no
/// model's name) left out.
fn from_limit(l: &Value) -> Option<Limit> {
    let kind = l.get("kind").and_then(Value::as_str)?.trim();
    let (base, scoped) = match kind.rsplit_once('_') {
        Some((base, "all")) => (base, false),
        Some((base, "scoped")) => (base, true),
        _ => (kind, false),
    };
    let title = period(base)?;
    let percent = number(l.get("percent")?)?.max(0.0);
    let name = l.get("scope").and_then(|s| s.get("model")).and_then(|m| m.get("display_name")).and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty());
    let model_id = l.get("scope").and_then(|s| s.get("model")).and_then(|m| m.get("id")).and_then(Value::as_str).unwrap_or("");
    let scope = if scoped {
        Some(name?.to_string())
    } else if title == SESSION {
        None
    } else {
        Some("all models".to_string())
    };
    let severity = l.get("severity").and_then(Value::as_str).and_then(Severity::parse).unwrap_or_else(|| Severity::of(percent));
    let window = match (title, scoped) {
        (SESSION, false) => Some("five_hour".to_string()),
        ("Weekly", false) => Some("seven_day".to_string()),
        ("Weekly", true) => family_window(&format!("{model_id} {}", name.unwrap_or(""))),
        _ => None,
    };
    Some(Limit {
        kind: kind.to_string(),
        title: title.to_string(),
        scope,
        scoped,
        percent,
        severity,
        resets_at: l.get("resets_at").and_then(when),
        active: l.get("is_active").and_then(Value::as_bool).unwrap_or(true),
        window,
    })
}

/// The weekly window Claude Code's rate-limit events name a model family's
/// limit by.
fn family_window(names: &str) -> Option<String> {
    let n = names.to_ascii_lowercase();
    if n.contains("opus") {
        Some("seven_day_opus".into())
    } else if n.contains("sonnet") {
        Some("seven_day_sonnet".into())
    } else {
        None
    }
}

/// One named window (`five_hour`, `seven_day`, `seven_day_opus`,
/// `seven_day_sonnet`) as a row, when it has a percent.
fn window_row(rl: &Value, key: &str) -> Option<Limit> {
    let w = rl.get(key).filter(|w| w.is_object())?;
    let percent = number(w.get("utilization")?)?.max(0.0);
    let (title, scope, scoped) = match key {
        "five_hour" => (SESSION, None, false),
        "seven_day" => ("Weekly", Some("all models"), false),
        "seven_day_opus" => ("Weekly", Some("Opus"), true),
        "seven_day_sonnet" => ("Weekly", Some("Sonnet"), true),
        _ => return None,
    };
    Some(Limit {
        kind: key.to_string(),
        title: title.to_string(),
        scope: scope.map(str::to_string),
        scoped,
        percent,
        severity: Severity::of(percent),
        resets_at: w.get("resets_at").and_then(when),
        // The named windows say nothing of it: a scoped one counts once used.
        active: !scoped || percent > 0.0,
        window: Some(key.to_string()),
    })
}

/// The named windows, when `limits[]` gives nothing: the session, the
/// week, each model's week (`model_scoped[]`, then Opus and Sonnet when it
/// hasn't them).
fn named_windows(rl: &Value) -> Vec<Limit> {
    let mut rows: Vec<Limit> = ["five_hour", "seven_day"].iter().filter_map(|k| window_row(rl, k)).collect();
    for m in rl.get("model_scoped").and_then(Value::as_array).into_iter().flatten() {
        let Some(name) = m.get("display_name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) else { continue };
        let Some(percent) = m.get("utilization").and_then(number) else { continue };
        let percent = percent.max(0.0);
        rows.push(Limit {
            kind: "model_scoped".into(),
            title: "Weekly".into(),
            scope: Some(name.to_string()),
            scoped: true,
            percent,
            severity: Severity::of(percent),
            resets_at: m.get("resets_at").and_then(when),
            active: percent > 0.0,
            window: family_window(name),
        });
    }
    for key in ["seven_day_opus", "seven_day_sonnet"] {
        if let Some(row) = window_row(rl, key) {
            let have = rows.iter().any(|r| r.scoped && r.window == row.window);
            if !have {
                rows.push(row);
            }
        }
    }
    rows
}

/// Extra usage, when it's on: `spend` (amounts in minor units with their
/// exponent), else `extra_usage` (minor units, `decimal_places`).
fn extra(rl: &Value) -> Option<Extra> {
    let spend = rl.get("spend").filter(|s| s.is_object());
    let eu = rl.get("extra_usage").filter(|e| e.is_object());
    let enabled = spend.and_then(|s| s.get("enabled")).and_then(Value::as_bool).or_else(|| eu.and_then(|e| e.get("is_enabled")).and_then(Value::as_bool)).unwrap_or(false);
    if !enabled {
        return None;
    }
    let money = |m: Option<&Value>| -> Option<(f64, String)> {
        let m = m.filter(|m| m.is_object())?;
        let minor = number(m.get("amount_minor")?)?;
        let exp = m.get("exponent").and_then(Value::as_u64).unwrap_or(2).min(6) as i32;
        Some((minor / 10f64.powi(exp), m.get("currency").and_then(Value::as_str).unwrap_or("").trim().to_string()))
    };
    if let Some((used, cur)) = spend.and_then(|s| money(s.get("used"))) {
        let limit = spend.and_then(|s| money(s.get("limit")));
        let currency = if cur.is_empty() { limit.as_ref().map(|l| l.1.clone()).unwrap_or_default() } else { cur };
        if !currency.is_empty() {
            return Some(Extra { used, limit: limit.map(|l| l.0).filter(|l| *l > 0.0), currency });
        }
    }
    let eu = eu?;
    let exp = eu.get("decimal_places").and_then(Value::as_u64).unwrap_or(2).min(6) as i32;
    let scale = 10f64.powi(exp);
    let used = number(eu.get("used_credits")?)? / scale;
    let limit = eu.get("monthly_limit").and_then(number).map(|l| l / scale).filter(|l| *l > 0.0);
    let currency = eu.get("currency").and_then(Value::as_str).map(str::trim).filter(|c| !c.is_empty())?.to_string();
    Some(Extra { used, limit, currency })
}

/// A number, or a number written as a string.
fn number(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok())).filter(|n| n.is_finite())
}

/// When a limit resets: an ISO time (`2026-11-14T03:00:00.412000+00:00`,
/// `…Z`, no offset read as UTC) or Unix seconds / milliseconds.
fn when(v: &Value) -> Option<i64> {
    if let Some(n) = v.as_f64() {
        return Some(if n > 1e12 { (n / 1000.0) as i64 } else { n as i64 });
    }
    let s = v.as_str()?.trim();
    if s.is_empty() {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|t| t.timestamp())
        .ok()
        .or_else(|| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").ok().map(|t| t.and_utc().timestamp()))
}

// ---- Live ----

/// A rate-limit event's utilization as a percent: Claude Code says a
/// fraction (1.0 at the limit, in the recordings); a value over 1 is read
/// as a percent already.
fn live_percent(u: f64) -> f64 {
    if u <= 1.0 {
        (u * 100.0).max(0.0)
    } else {
        u
    }
}

impl PlanUsage {
    /// **A turn's `rate_limit_event`**: the rows it names
    /// (`rateLimitType`, `unifiedWindows`) take its numbers; rejected is
    /// critical, a warning at least a warning. Whether anything changed.
    pub fn apply(&mut self, r: &RateLimit) -> bool {
        let mut changed = false;
        if let Some(kind) = r.kind.as_deref() {
            changed |= self.set_window(kind, r.utilization, r.resets_at, Some(&r.status));
        }
        for w in &r.windows {
            if r.kind.as_deref() != Some(w.kind.as_str()) {
                changed |= self.set_window(&w.kind, w.utilization, w.resets_at, None);
            }
        }
        changed
    }

    fn set_window(&mut self, kind: &str, utilization: Option<f64>, resets_at: Option<i64>, status: Option<&str>) -> bool {
        let mut changed = false;
        for row in self.limits.iter_mut().filter(|l| l.window.as_deref() == Some(kind)) {
            if let Some(p) = utilization.filter(|u| u.is_finite()).map(live_percent) {
                if (row.percent - p).abs() >= 0.05 {
                    row.percent = p;
                    changed = true;
                }
            }
            if let Some(at) = resets_at.filter(|at| row.resets_at != Some(*at)) {
                row.resets_at = Some(at);
                changed = true;
            }
            let severity = match status {
                Some("rejected") => Severity::Critical,
                Some("allowed_warning") => row.severity.max(Severity::Warning),
                _ => row.severity,
            };
            if severity != row.severity {
                row.severity = severity;
                changed = true;
            }
        }
        changed
    }
}

// ---- Asking ----

/// Read `get_usage`'s answer (request `id`) off a session's events, `wait`
/// at most. Other events meanwhile are passed over (between turns there
/// are none).
pub async fn read_answer(rx: &mut UnboundedReceiver<Event>, id: &str, wait: Duration) -> Asked {
    let got = tokio::time::timeout(wait, async {
        while let Some(e) = rx.recv().await {
            match e {
                Event::ControlResponse(c) if c.request_id == id || c.request_id.is_empty() => return from_response(&c),
                Event::Exited(_) => return Asked::NoAnswer,
                _ => {}
            }
        }
        Asked::NoAnswer
    })
    .await;
    got.unwrap_or(Asked::NoAnswer)
}

/// Ask a live session (its events on `rx`, held by the caller: no turn is
/// reading them).
pub async fn ask_session(session: &Session, rx: &mut UnboundedReceiver<Event>, wait: Duration) -> Asked {
    match session.get_usage() {
        Ok(id) => read_answer(rx, &id, wait).await,
        Err(_) => Asked::NoAnswer,
    }
}

/// The one-off process's run: no tools, no MCP server, none of the
/// person's settings, nothing saved, on the plan.
pub fn probe_spec(cwd: PathBuf) -> Spec {
    let mut spec = Spec::new(cwd, Pay::Plan);
    spec.tools = Some(Vec::new());
    spec.strict_mcp_config = true;
    spec.setting_sources = Some(Vec::new());
    spec.persist = false;
    spec
}

/// **A one-off probe** with `program` (Claude Code, or the stand-in): the
/// request alone, its answer read (`wait` at most), then stdin closed — and
/// the process ended at once when it didn't answer. Always ended.
pub async fn probe_with(program: &Program, cwd: PathBuf, wait: Duration) -> Asked {
    let Ok((session, mut rx)) = Session::start_with(program, probe_spec(cwd)) else { return Asked::NoAnswer };
    let asked = ask_session(&session, &mut rx, wait).await;
    session.end();
    if asked == Asked::NoAnswer {
        session.kill();
    }
    // Dropped: a process that lingers is ended after a moment.
    asked
}

/// [`probe_with`] the person's Claude Code ([`cli::find`]).
pub async fn probe(cwd: PathBuf, wait: Duration) -> Asked {
    match cli::find() {
        Some(program) => probe_with(&program, cwd, wait).await,
        None => Asked::NoAnswer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::claude_code::events::{parse_line, RateWindow};

    fn fixture() -> Value {
        let text = include_str!("fixtures/2.1.296/synthetic-get-usage.json");
        serde_json::from_str(text).unwrap()
    }

    fn row<'a>(u: &'a PlanUsage, title: &str, scope: Option<&str>) -> &'a Limit {
        u.limits.iter().find(|l| l.title == title && l.scope.as_deref() == scope).unwrap_or_else(|| panic!("no {title} {scope:?} in {u:?}"))
    }

    fn at(iso: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(iso).unwrap().timestamp()
    }

    #[test]
    fn the_recorded_answer_reads_as_rows_and_extra_usage() {
        let line = fixture().to_string();
        let Some(Event::ControlResponse(c)) = parse_line(&line) else { panic!("a control response") };
        assert_eq!(c.request_id, "yap_1_0a1b2c3d");
        let Asked::Answered(Some(u)) = from_response(&c) else { panic!("{:?}", from_response(&c)) };
        assert_eq!(u.subscription.as_deref(), Some("max"));
        // limits[] first; its unknown kind and the inactive scoped one at
        // 0 % left out; the five-hour window added as the session.
        let labels: Vec<(String, Option<String>, f64)> = u.limits.iter().map(|l| (l.title.clone(), l.scope.clone(), l.percent)).collect();
        assert_eq!(
            labels,
            [
                ("Current session".to_string(), None, 23.0),
                ("Weekly".to_string(), Some("all models".to_string()), 9.0),
                ("Weekly".to_string(), Some("Fable".to_string()), 4.0),
            ]
        );
        let weekly = row(&u, "Weekly", Some("all models"));
        assert_eq!((weekly.kind.as_str(), weekly.severity, weekly.active, weekly.scoped), ("weekly_all", Severity::Normal, true, false));
        assert_eq!(weekly.resets_at, Some(at("2026-11-14T03:00:00Z")));
        assert_eq!(row(&u, "Current session", None).resets_at, Some(at("2026-11-07T15:00:00Z")));
        assert!(row(&u, "Weekly", Some("Fable")).scoped);
        // Extra usage from `spend`: €3.40 of €25.00.
        assert_eq!(u.extra, Some(Extra { used: 3.4, limit: Some(25.0), currency: "EUR".into() }));
        // What the page gets: no window, camelCase.
        let v = serde_json::to_value(&u).unwrap();
        assert_eq!(v["limits"][1], json!({"kind":"weekly_all","title":"Weekly","scope":"all models","scoped":false,"percent":9.0,"severity":"normal","resetsAt":at("2026-11-14T03:00:00Z"),"active":true}));
        assert_eq!(v["extra"], json!({"used":3.4,"limit":25.0,"currency":"EUR"}));
    }

    #[test]
    fn the_body_may_come_one_level_up() {
        let inner = fixture()["response"]["response"].clone();
        let whole = json!({"subtype":"success","request_id":"r1"}).as_object().unwrap().clone().into_iter().chain(inner.as_object().unwrap().clone()).collect::<serde_json::Map<_, _>>();
        let line = json!({"type":"control_response","response": whole}).to_string();
        let Some(Event::ControlResponse(c)) = parse_line(&line) else { panic!() };
        let Asked::Answered(Some(u)) = from_response(&c) else { panic!() };
        assert_eq!(u.limits.len(), 3);
        assert_eq!(parse(&inner), Some(u.clone()));
        assert_eq!(parse(&json!({"response": inner})), Some(u));
    }

    #[test]
    fn an_api_keys_answer_or_a_refusal_has_no_numbers() {
        let key = json!({"subscription_type": null, "rate_limits_available": false, "rate_limits": null});
        assert_eq!(parse(&key), None);
        let line = json!({"type":"control_response","response":{"subtype":"success","request_id":"r2","response":key}}).to_string();
        let Some(Event::ControlResponse(c)) = parse_line(&line) else { panic!() };
        assert_eq!(from_response(&c), Asked::Answered(None));
        // An older Claude Code without get_usage.
        let refused = r#"{"type":"control_response","response":{"subtype":"error","request_id":"r3","error":"Unsupported control request subtype: get_usage"}}"#;
        let Some(Event::ControlResponse(c)) = parse_line(refused) else { panic!() };
        assert_eq!(from_response(&c), Asked::Refused);
        // Something else altogether, or nothing.
        assert_eq!(parse(&json!({"still_queued": []})), None);
        assert_eq!(parse(&json!({"rate_limits_available": true, "rate_limits": {}})), None, "no rows: nothing to show");
        assert_eq!(parse(&Value::Null), None);
    }

    #[test]
    fn without_limits_the_named_windows_are_read() {
        let mut v = fixture()["response"]["response"].clone();
        v["rate_limits"].as_object_mut().unwrap().remove("limits");
        v["rate_limits"]["spend"] = Value::Null;
        let u = parse(&v).unwrap();
        let labels: Vec<(String, Option<String>, f64, bool)> = u.limits.iter().map(|l| (l.title.clone(), l.scope.clone(), l.percent, l.active)).collect();
        assert_eq!(
            labels,
            [
                ("Current session".to_string(), None, 23.0, true),
                ("Weekly".to_string(), Some("all models".to_string()), 9.0, true),
                ("Weekly".to_string(), Some("Fable".to_string()), 4.0, true),
            ],
            "seven_day_opus at 0 % isn't shown; Sonnet's is null"
        );
        assert_eq!(row(&u, "Weekly", Some("all models")).window.as_deref(), Some("seven_day"));
        // Extra usage from `extra_usage` (minor units, decimal places).
        assert_eq!(u.extra, Some(Extra { used: 3.4, limit: Some(25.0), currency: "EUR".into() }));
        // Opus used this week: its own row.
        v["rate_limits"]["seven_day_opus"] = json!({"utilization": 41.5, "resets_at": 1_794_067_200});
        let u = parse(&v).unwrap();
        let opus = row(&u, "Weekly", Some("Opus"));
        assert_eq!((opus.percent, opus.resets_at, opus.window.as_deref()), (41.5, Some(1_794_067_200), Some("seven_day_opus")));
        // limits[] with nothing Yap can place: the named windows instead.
        v["rate_limits"]["limits"] = json!([{"kind": "quokka_window", "percent": 50}]);
        assert_eq!(parse(&v).unwrap().limits.len(), 4);
    }

    #[test]
    fn unknown_kinds_and_odd_fields_are_left_out_never_an_error() {
        let v = json!({"rate_limits_available": true, "rate_limits": {
            "limits": [
                {"kind": "weekly_all", "percent": "71", "severity": "warning", "resets_at": "2026-11-14T03:00:00Z"},
                {"kind": "weekly_scoped", "percent": 12, "scope": null},
                {"kind": "weekly_scoped", "percent": 30, "scope": {"model": {"display_name": "Sonnet"}}, "severity": "fuchsia"},
                {"kind": "daily_all", "percent": 101, "resets_at": 1_794_067_200_000_i64, "is_active": false},
                {"kind": "session", "percent": null},
                {"kind": 7, "percent": 3},
                {"percent": 3},
                "not an object",
                {"kind": "weekly_all"}
            ],
            "extra_usage": {"is_enabled": false, "used_credits": 100, "currency": "GBP"},
            "five_hour": {"utilization": "nope"}
        }});
        let u = parse(&v).unwrap();
        let labels: Vec<(String, Option<String>)> = u.limits.iter().map(|l| (l.title.clone(), l.scope.clone())).collect();
        assert_eq!(
            labels,
            [("Daily".to_string(), Some("all models".to_string())), ("Weekly".to_string(), Some("all models".to_string())), ("Weekly".to_string(), Some("Sonnet".to_string()))],
            "no scoped row without its model's name, no row without a percent"
        );
        let daily = row(&u, "Daily", Some("all models"));
        assert_eq!((daily.severity, daily.resets_at, daily.active), (Severity::Critical, Some(1_794_067_200), false), "past the limit: critical; milliseconds read");
        assert_eq!(row(&u, "Weekly", Some("all models")).severity, Severity::Warning);
        assert_eq!(row(&u, "Weekly", Some("Sonnet")).severity, Severity::Normal, "an unknown word: from its percent");
        assert_eq!(row(&u, "Weekly", Some("Sonnet")).window.as_deref(), Some("seven_day_sonnet"));
        assert_eq!(u.extra, None, "extra usage off");
        assert_eq!(u.subscription, None);
        // No limit set on extra usage: what's spent alone.
        let v = json!({"rate_limits": {"spend": {"enabled": true, "used": {"amount_minor": 1250, "currency": "JPY", "exponent": 0}, "limit": null}}});
        assert_eq!(parse(&v).unwrap().extra, Some(Extra { used: 1250.0, limit: None, currency: "JPY".into() }));
    }

    #[test]
    fn a_turns_rate_limit_events_move_the_rows_they_name() {
        let mut u = parse(&fixture()["response"]).unwrap();
        let before = u.clone();
        // A warning on the week: 86 %, a warning at least.
        let warn = RateLimit { status: "allowed_warning".into(), kind: Some("seven_day".into()), utilization: Some(0.86), ..Default::default() };
        assert!(u.apply(&warn));
        let w = row(&u, "Weekly", Some("all models"));
        assert_eq!((w.percent.round(), w.severity), (86.0, Severity::Warning));
        assert_eq!(row(&u, "Current session", None).percent, 23.0, "the others as they were");
        assert!(!u.apply(&warn), "the same again: nothing new");
        // Every window in `unifiedWindows`; the session rejected.
        let rejected = RateLimit {
            status: "rejected".into(),
            kind: Some("five_hour".into()),
            utilization: Some(1.0),
            resets_at: Some(1_794_067_200),
            windows: vec![RateWindow { kind: "seven_day".into(), utilization: Some(0.9), resets_at: None }, RateWindow { kind: "overage".into(), utilization: Some(0.1), resets_at: None }],
            ..Default::default()
        };
        assert!(u.apply(&rejected));
        let s = row(&u, "Current session", None);
        assert_eq!((s.percent, s.severity, s.resets_at), (100.0, Severity::Critical, Some(1_794_067_200)));
        assert_eq!(row(&u, "Weekly", Some("all models")).percent.round(), 90.0);
        // A type no row is: nothing.
        let other = RateLimit { status: "allowed".into(), kind: Some("seven_day_opus".into()), utilization: Some(0.5), ..Default::default() };
        assert!(!u.apply(&other));
        assert_ne!(u, before);
    }

    #[tokio::test]
    async fn the_answer_is_read_by_its_id_and_a_silent_one_times_out() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let answer = |id: &str| {
            let mut v = fixture();
            v["response"]["request_id"] = json!(id);
            parse_line(&v.to_string()).unwrap()
        };
        // Another request's receipt first, then this one's.
        tx.send(parse_line(r#"{"type":"control_response","response":{"subtype":"success","request_id":"other","response":{"still_queued":[]}}}"#).unwrap()).unwrap();
        tx.send(answer("mine")).unwrap();
        let Asked::Answered(Some(u)) = read_answer(&mut rx, "mine", Duration::from_secs(5)).await else { panic!() };
        assert_eq!(u.limits.len(), 3);
        // Nothing comes: no answer, in time.
        let started = std::time::Instant::now();
        assert_eq!(read_answer(&mut rx, "late", Duration::from_millis(200)).await, Asked::NoAnswer);
        assert!(started.elapsed() < Duration::from_secs(3));
        // The process ended first.
        tx.send(Event::Exited(Some(0))).unwrap();
        assert_eq!(read_answer(&mut rx, "late", Duration::from_secs(5)).await, Asked::NoAnswer);
    }

    #[test]
    fn the_request_and_the_probes_run() {
        assert_eq!(request(), json!({"subtype":"get_usage","skip_behaviors":true}));
        let spec = probe_spec(std::env::temp_dir());
        let a = super::super::session::args(&spec, &Default::default());
        let after = |flag: &str| a.iter().position(|x| x == flag).map(|i| a[i + 1].as_str());
        assert_eq!(after("--tools"), Some(""));
        assert_eq!(after("--setting-sources"), Some(""));
        assert!(a.contains(&"--strict-mcp-config".to_string()));
        assert!(a.contains(&"--no-session-persistence".to_string()), "nothing saved");
        assert!(!a.contains(&"--mcp-config".to_string()) && !a.contains(&"--model".to_string()));
        assert_eq!(spec.pay, Pay::Plan);
    }
}

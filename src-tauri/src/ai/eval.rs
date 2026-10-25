//! **Model evaluations** (`yap.exe eval …`, debug builds only; see
//! `docs/model-evals.md`): put real models — Groq, OpenAI, Anthropic, a GGUF
//! on Yap's own llamafile, any OpenAI-compatible server — through Chat's own
//! code path (`turn::chat_turn`: the open-apps fast path, the tool loop,
//! the plain chat + RAG fallback for models without tools) over the scenarios
//! in `evals/scenarios/*.json`, and get everything back: every request (tool
//! schemas included, keys redacted), every response, each tool call and its
//! result, timings, tokens, rate-limit waits, and a pass/fail per scenario
//! with the reasons.
//!
//! **Side effects stay inside the run.** The run has its own data dir
//! (`portable::use_data_dir`, under the run's folder), its settings live in
//! memory ([`config`]: an API key never reaches a file), notes are an
//! in-memory store reset per scenario (`notes::eval_reset`), apps come from a
//! stand-in list (or this PC's Start menu, read-only) and "opening" one only
//! records it (test mode, `e2e::active`, as in the e2e suite), and the
//! clipboard tool is recorded instead of run ([`intercept_tool`]). No window,
//! audio, browser or launch.
//!
//! This file holds the hooks the app's own code calls (`llm.rs`, `tools.rs`,
//! `config.rs`, `notes.rs`); each is a no-op unless a run is going, and always
//! in release builds. The harness itself lives in `eval/` (debug builds).

use std::time::Duration;

use serde_json::Value;

#[cfg(debug_assertions)]
mod brief;
#[cfg(debug_assertions)]
mod cleanup;
#[cfg(debug_assertions)]
mod languages;
#[cfg(debug_assertions)]
mod library;
#[cfg(debug_assertions)]
mod local;
#[cfg(debug_assertions)]
mod record;
#[cfg(debug_assertions)]
mod runner;
#[cfg(debug_assertions)]
mod scenario;
#[cfg(debug_assertions)]
mod search;
#[cfg(debug_assertions)]
mod serving;
#[cfg(debug_assertions)]
mod speakers;
#[cfg(debug_assertions)]
mod spike;
#[cfg(debug_assertions)]
mod standin;
#[cfg(debug_assertions)]
mod transforms;
#[cfg(debug_assertions)]
mod styles;
#[cfg(debug_assertions)]
mod units;

#[cfg(debug_assertions)]
static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A model evaluation is running in this process (`yap.exe eval`).
pub fn active() -> bool {
    #[cfg(debug_assertions)]
    {
        ACTIVE.load(std::sync::atomic::Ordering::SeqCst)
    }
    #[cfg(not(debug_assertions))]
    false
}

/// `yap.exe eval …`: run the harness with these arguments (after "eval").
/// Returns the process's exit code.
pub fn run(args: Vec<String>) -> i32 {
    #[cfg(debug_assertions)]
    {
        runner::main(args)
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = args;
        eprintln!("yap.exe eval is in debug builds only (see docs/model-evals.md).");
        2
    }
}

/// How far a run's clock is ahead of the real one, seconds (`--clock-step`:
/// each turn of a scenario a few minutes after the one before, as in a real
/// chat — what Chat tells the model the time is, `turn::chat_turn`).
#[cfg(debug_assertions)]
static CLOCK_AHEAD: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// Now, as Chat tells the model: the real time, or a run's clock
/// (`--clock-step`).
pub fn now() -> chrono::DateTime<chrono::Utc> {
    #[cfg(debug_assertions)]
    {
        if active() {
            return chrono::Utc::now() + chrono::Duration::seconds(CLOCK_AHEAD.load(std::sync::atomic::Ordering::SeqCst));
        }
    }
    chrono::Utc::now()
}

/// Move a run's clock on by `secs` (the runner, before each turn).
#[cfg(debug_assertions)]
pub(crate) fn clock_forward(secs: i64) {
    CLOCK_AHEAD.fetch_add(secs, std::sync::atomic::Ordering::SeqCst);
}

/// The settings a run is on (`config::load` returns them): `None` outside a
/// run.
pub fn config() -> Option<crate::config::YapConfig> {
    #[cfg(debug_assertions)]
    {
        if active() {
            return record::config();
        }
    }
    None
}

/// `--tools on|off` (or a scenario's own `tools`): whether Chat's model is
/// treated as able to call tools, whatever `tools::supports_tools` would say.
pub fn tools_forced() -> Option<bool> {
    #[cfg(debug_assertions)]
    {
        if active() {
            return record::tools_forced();
        }
    }
    None
}

/// 0: no local model in this run (or not known), 1: on the processor, 2: on
/// the graphics card (`--ngl`).
#[cfg(debug_assertions)]
static LOCAL_ON_GPU: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// A run's local model: on the graphics card (`Some(true)`, `--ngl` > 0) or
/// the processor; `None` outside a run or for a cloud model. (The app reads
/// the sidecar's placement instead: `local_llm::placement`.)
pub fn local_on_gpu() -> Option<bool> {
    #[cfg(debug_assertions)]
    {
        if active() {
            return match LOCAL_ON_GPU.load(std::sync::atomic::Ordering::SeqCst) {
                1 => Some(false),
                2 => Some(true),
                _ => None,
            };
        }
    }
    None
}

/// The runner, when a local model starts.
#[cfg(debug_assertions)]
pub(crate) fn set_local_on_gpu(on_gpu: Option<bool>) {
    let v = match on_gpu {
        None => 0,
        Some(false) => 1,
        Some(true) => 2,
    };
    LOCAL_ON_GPU.store(v, std::sync::atomic::Ordering::SeqCst);
}

/// `--timeout <secs>`: how long a model call may take in this run, instead
/// of the caller's own `timeout` (a slow local model on a CPU would
/// otherwise just time out, which a run can also measure by leaving it).
pub fn timeout(timeout: Duration) -> Duration {
    #[cfg(debug_assertions)]
    {
        if active() {
            return record::timeout().unwrap_or(timeout);
        }
    }
    timeout
}

/// Web search in this scenario goes to the real services and wikis (`web.rs`
/// `Endpoints::current`), although a run is in test mode — a scenario with
/// a stand-in web says no, and gets the stand-in instead.
pub fn web_live() -> bool {
    #[cfg(debug_assertions)]
    {
        if active() {
            return record::web_live();
        }
    }
    false
}

/// A tool call the run carries out itself instead of the app: the clipboard
/// (recorded, never touched). `None`: the app runs it as usual.
pub fn intercept_tool(name: &str, args: &Value) -> Option<Value> {
    #[cfg(debug_assertions)]
    {
        if active() {
            return record::intercept_tool(name, args);
        }
    }
    let _ = (name, args);
    None
}

/// A model call's request body as a run wants it: `--temperature`, `--set
/// key=json` and a bench's own fields (llama.cpp's `id_slot`) laid over
/// what the app built. Does nothing outside a run.
pub fn adjust_body(body: &mut Value) {
    #[cfg(debug_assertions)]
    {
        if active() {
            record::adjust_body(body);
        }
    }
    let _ = body;
}

/// Every tool call of the chat's tool loop, with its result (the run logs
/// it and scores it).
pub fn tool_called(name: &str, args: &Value, result: &Value) {
    #[cfg(debug_assertions)]
    {
        if active() {
            record::tool_called(name, args, result);
        }
    }
    let _ = (name, args, result);
}

/// Yap changed the model's answer before showing it, or searched for it
/// (`turn::chat_turn`'s post-checks: "stripped_citations",
/// "stripped_sources_note", "not_searched_note", "yap_search"): logged, and a
/// scenario's `no_post_fixes` warns about it.
pub fn post_fix(what: &str) {
    #[cfg(debug_assertions)]
    {
        if active() {
            record::post_fix(what);
        }
    }
    let _ = what;
}

/// The rate-limit headers of a response (`x-ratelimit-*`, `retry-after*`),
/// for the run's log and its pacing. `Null` outside a run.
pub fn limit_headers(headers: &reqwest::header::HeaderMap) -> Value {
    #[cfg(debug_assertions)]
    {
        if active() {
            return record::limit_headers(headers);
        }
    }
    let _ = headers;
    Value::Null
}

/// One model call (`llm::post_chat_request`), as a run sees it: paced to the
/// run's tokens-per-minute budget before it goes, and every attempt, refusal,
/// wait and answer logged. Outside a run every method does nothing.
pub struct Call {
    #[cfg(debug_assertions)]
    inner: Option<record::CallLog>,
}

impl Call {
    /// Before a call: wait for the run's pacing, then start its log.
    pub async fn start(provider: &str, endpoint: &str, body: &Value) -> Call {
        #[cfg(debug_assertions)]
        {
            if active() {
                return Call { inner: Some(record::CallLog::start(provider, endpoint, body).await) };
            }
            Call { inner: None }
        }
        #[cfg(not(debug_assertions))]
        {
            let _ = (provider, endpoint, body);
            Call {}
        }
    }

    /// An attempt goes out with `body`.
    pub fn send(&self, body: &Value) {
        #[cfg(debug_assertions)]
        if let Some(c) = &self.inner {
            c.send(body);
        }
        let _ = body;
    }

    /// The provider said no (HTTP `status`, its body `detail`).
    pub fn refused(&self, status: u16, limits: &Value, detail: &str) {
        #[cfg(debug_assertions)]
        if let Some(c) = &self.inner {
            c.refused(status, limits, detail);
        }
        let _ = (status, limits, detail);
    }

    /// A rate limit is being waited out before the next attempt.
    pub fn waiting(&self, wait: Duration) {
        #[cfg(debug_assertions)]
        if let Some(c) = &self.inner {
            c.waiting(wait);
        }
        let _ = wait;
    }

    /// The call failed with `error`, which is handed back as it was.
    pub fn failed(&self, error: String) -> String {
        #[cfg(debug_assertions)]
        if let Some(c) = &self.inner {
            c.failed(&error);
        }
        error
    }

    /// The answer came (the whole response body).
    pub fn answered(&self, limits: &Value, body: &Value) {
        #[cfg(debug_assertions)]
        if let Some(c) = &self.inner {
            c.answered(limits, body);
        }
        let _ = (limits, body);
    }
}

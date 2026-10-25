//! **The embedding model**: EmbeddingGemma-300M (Q8_0 GGUF, 334 MB, 768
//! dimensions, 100+ languages), run by Yap's own llamafile runtime as a
//! second, small server in `--embedding` mode — the on-device chat model's
//! engine, so nothing new to ship: on the graphics card where there is one
//! (CUDA on NVIDIA, Vulkan elsewhere; ~390 MiB), else the processor.
//!
//! **Why this, measured** (`yap-private/docs/local-ai-research.md` #7, the
//! model evaluations' `--bench embed`): over paraphrased questions recall@1
//! 1.00 against Chat's keyword search's 0.12; 64 note-sized passages in 1.4 s
//! on an RTX 5070 Ti, 121 s on its processor (~370 tokens/s), a query in
//! 8–14 ms. Qwen3-Embedding-0.6B read as well at twice the cost (2.8 GB on
//! the card). ONNX through `ort` was the other way: it's only in `engines`
//! builds, and Gemma's tokenizer would be a new dependency; llamafile
//! tokenizes itself and is the same in every build.
//!
//! **Never in the cloud**: the server listens on 127.0.0.1, its requests go
//! through the network receipts as "on this PC" (`privacy/net.rs`,
//! [`Purpose::Embeddings`](crate::privacy::net::Purpose)). It starts when the
//! index or a question needs it and stops after [`IDLE`] unused, runs below
//! normal priority on half the processor's threads, and dies with Yap
//! (`procs::end_with_yap`; the orphan sweep finds it as a `llamafile.exe`).
//!
//! **Install** ([`install`]): the runtime if it isn't there (the on-device
//! model's own download, `local_llm::models::ensure_runtime`), then the GGUF
//! into `<data>/models/search/` — size and SHA-256 checked, a partial
//! download resumed, the Gemma Terms' notice beside it. A test run downloads
//! only stand-ins from 127.0.0.1 (`YAP_LLM_SOURCES`, key `embeddinggemma`)
//! and talks to a stand-in server (`YAP_E2E_EMBED_URL`), never llamafile.

use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use crate::privacy::net::Recorded as _;

/// The GGUF, pinned to ggml-org's conversion (its commit and LFS oid).
pub const MODEL_FILE: &str = "embeddinggemma-300M-Q8_0.gguf";
pub const MODEL_URL: &str =
    "https://huggingface.co/ggml-org/embeddinggemma-300M-GGUF/resolve/0f741b5a6585bd53aeb15cd1372c56f2a0f65e12/embeddinggemma-300M-Q8_0.gguf";
pub const MODEL_SHA256: &str = "b5ce9d77a3fc4b3b39ccb5643c36777911cc4eb46a66962eadfa3f5f60490d63";
pub const MODEL_BYTES: u64 = 333_590_944;
/// What Settings calls it.
pub const MODEL_NAME: &str = "EmbeddingGemma 300M";
pub const LICENSE: &str = "Gemma Terms of Use";
pub const LICENSE_URL: &str = "https://ai.google.dev/gemma/terms";
/// The model as the index knows it: another model (or another quantisation)
/// means another index.
pub const MODEL_TAG: &str = "embeddinggemma-300m-q8_0";
pub const DIMS: usize = 768;

/// Written beside the model.
const NOTICE: &str = "EmbeddingGemma 300M (Q8_0 GGUF, ggml-org/embeddinggemma-300M-GGUF)\n\
Gemma is provided under and subject to the Gemma Terms of Use found at\n\
https://ai.google.dev/gemma/terms, and its Prohibited Use Policy at\n\
https://ai.google.dev/gemma/prohibited_use_policy.\n\
Yap uses it on this PC only, to search your meetings and notes by meaning.\n";

/// Stopped after this long unused.
pub const IDLE: Duration = Duration::from_secs(120);
/// How long a start may take (a first run waits for the antivirus).
const START_WAIT: Duration = Duration::from_secs(120);
/// A start that failed this many times this session isn't tried again
/// (until the model is downloaded again or the index rebuilt).
const MAX_FAILED_STARTS: u32 = 3;

/// EmbeddingGemma's document prompt.
pub fn document(title: &str, text: &str) -> String {
    let title = if title.trim().is_empty() { "none" } else { title.trim() };
    format!("title: {title} | text: {text}")
}

/// …and its query prompt.
pub fn query(q: &str) -> String {
    format!("task: search result | query: {}", q.trim())
}

/// Where the model lives: `<data>/models/search/`.
pub fn dir() -> PathBuf {
    crate::config::data_dir().join("models").join("search")
}

pub fn model_path() -> PathBuf {
    dir().join(MODEL_FILE)
}

/// The model's download: pinned, or a test run's stand-in (`YAP_LLM_SOURCES`,
/// `{"embeddinggemma": {url, sha256, size}}`, debug builds).
fn source() -> crate::speakers::diarize::registry::Source {
    let (url, sha256) = crate::ai::local_llm::models::source("embeddinggemma", MODEL_URL, MODEL_SHA256);
    #[cfg_attr(not(debug_assertions), allow(unused_mut))]
    let mut size = MODEL_BYTES;
    #[cfg(debug_assertions)]
    if url != MODEL_URL {
        size = std::env::var_os("YAP_LLM_SOURCES")
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v["embeddinggemma"]["size"].as_u64())
            .unwrap_or(MODEL_BYTES);
    }
    crate::speakers::diarize::registry::Source { url, sha256, size, unpack: crate::speakers::diarize::registry::Unpack::File(MODEL_FILE) }
}

/// The model is on disk, whole.
pub fn model_installed() -> bool {
    std::fs::metadata(model_path()).is_ok_and(|m| m.len() == source().size)
}

/// The engine (llamafile) is on disk.
pub fn runtime_installed() -> bool {
    crate::ai::local_llm::runtime_path().is_file()
}

/// Both are there (or a model evaluation brought its own server, [`use_server`]).
pub fn installed() -> bool {
    #[cfg(debug_assertions)]
    if eval_server().is_some() {
        return true;
    }
    model_installed() && runtime_installed()
}

/// A model evaluation's own embedding server (`eval/search.rs`), which every
/// search uses instead of starting Yap's: debug builds.
#[cfg(debug_assertions)]
static EVAL_SERVER: Mutex<Option<String>> = Mutex::new(None);

#[cfg(debug_assertions)]
pub fn use_server(url: Option<String>) {
    *EVAL_SERVER.lock().unwrap_or_else(|p| p.into_inner()) = url;
}

#[cfg(debug_assertions)]
fn eval_server() -> Option<String> {
    EVAL_SERVER.lock().ok().and_then(|g| g.clone())
}

/// What a download fetches now (bytes): the model, and the engine unless
/// the on-device model already brought it.
pub fn download_bytes() -> u64 {
    let mut n = 0;
    if !model_installed() {
        n += source().size;
    }
    if !runtime_installed() {
        n += crate::ai::local_llm::runtime::RUNTIME_BYTES;
    }
    n
}

static INSTALLING: AtomicBool = AtomicBool::new(false);

/// A download under way.
pub fn installing() -> bool {
    INSTALLING.load(Ordering::SeqCst)
}

/// Progress of the model's download (`yap-search-download`; the engine's is
/// `local-llm-download-progress` with model "search").
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    stage: &'static str,
    percent: u8,
    downloaded_bytes: u64,
    total_bytes: u64,
}

/// Download what's missing: the engine, then the model. One at a time.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return Err("The search model is already downloading".to_string());
    }
    let result = install_inner(app).await;
    INSTALLING.store(false, Ordering::SeqCst);
    if result.is_ok() {
        FAILED_STARTS.store(0, Ordering::SeqCst);
    }
    result
}

async fn install_inner(app: &AppHandle) -> Result<(), String> {
    use crate::speakers::diarize::registry as dl;
    if !runtime_installed() {
        crate::ai::local_llm::models::ensure_runtime("search", Some(app)).await?;
    }
    if model_installed() {
        return Ok(());
    }
    let src = source();
    if crate::shell::e2e::active() && !crate::ai::local_llm::models::allowed_in_test_run(&src.url) {
        return Err("Test runs never download models from the internet".to_string());
    }
    let dir = dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make the search model's folder: {e}"))?;
    let free = crate::ai::local_llm::runtime::free_bytes(&dir);
    if !crate::ai::local_llm::runtime::room_for(free, src.size + 64 * 1_048_576) {
        return Err(format!(
            "Not enough disk space: the search model needs {} MB, {} MB are free",
            src.size / 1_048_576 + 64,
            free.unwrap_or(0) / 1_048_576
        ));
    }
    let partial = dir.join(format!("{MODEL_FILE}.partial"));
    tracing::info!(bytes = src.size, "Meeting search: downloading the search model");
    let total = src.size;
    let last = std::sync::atomic::AtomicU8::new(255);
    let report = |got: u64| {
        let pct = (got * 100).checked_div(total).map_or(100, |p| p.min(100) as u8);
        if last.swap(pct, Ordering::Relaxed) != pct {
            let _ = app.emit("yap-search-download", Progress { stage: "model", percent: pct, downloaded_bytes: got, total_bytes: total });
        }
    };
    dl::download(&src, &partial, &report, crate::privacy::net::Download::SearchModel).await?;
    dl::verify(&partial, &src).await?;
    std::fs::rename(&partial, model_path()).map_err(|e| format!("Couldn't put the search model in place: {e}"))?;
    let _ = std::fs::write(dir.join("NOTICE.txt"), NOTICE);
    tracing::info!("Meeting search: the search model is installed");
    Ok(())
}

/// Delete the model (the engine stays: the on-device model may use it).
pub fn delete_model() -> Result<(), String> {
    stop();
    let dir = dir();
    for f in [MODEL_FILE.to_string(), format!("{MODEL_FILE}.partial"), "NOTICE.txt".into(), "embedder.log".into()] {
        let p = dir.join(f);
        if p.exists() {
            std::fs::remove_file(&p).map_err(|e| format!("Couldn't delete the search model: {e}"))?;
        }
    }
    let _ = std::fs::remove_dir(&dir);
    tracing::info!("Meeting search: the search model was deleted");
    Ok(())
}

// ---- the server -------------------------------------------------------------

struct Running {
    child: Child,
    url: String,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
static STARTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
/// When it was last used (unix ms).
static LAST_USE: AtomicU64 = AtomicU64::new(0);
/// Requests under way: never stopped under one.
static IN_FLIGHT: AtomicU32 = AtomicU32::new(0);
static FAILED_STARTS: AtomicU32 = AtomicU32::new(0);
static WATCHING: AtomicBool = AtomicBool::new(false);
/// Yap is quitting: nothing starts.
static CLOSING: AtomicBool = AtomicBool::new(false);
/// Where its layers went at the last start ("gpu" | "partial" | "cpu").
static ON: Mutex<Option<&'static str>> = Mutex::new(None);

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Where the model ran at its last start: "gpu" | "partial" | "cpu".
pub fn runs_on() -> Option<&'static str> {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        return stand_in().map(|_| "cpu");
    }
    ON.lock().ok().and_then(|g| *g)
}

/// The server's arguments: the sidecar's runtime in embedding mode, the
/// context and batches for passages of a few hundred tokens, all layers on
/// the card that fit (`ngl` 999; none on a PC without one), half the
/// processor's threads. Pure.
pub fn server_args(model: &Path, port: u16, threads: usize) -> Vec<String> {
    let t = threads.to_string();
    [
        "--server", "--host", "127.0.0.1", "--port", &port.to_string(), "-m", &model.to_string_lossy(), "--embedding", "-c", "4096", "-b",
        "2048", "-ub", "2048", "-ngl", "999", "-t", &t, "-tb", &t, "-lv", "4",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Half the processor's threads, at least 2.
fn threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get() / 2).unwrap_or(2).max(2)
}

/// The test run's stand-in server (`YAP_E2E_EMBED_URL`, debug builds).
#[cfg(debug_assertions)]
fn stand_in() -> Option<String> {
    std::env::var("YAP_E2E_EMBED_URL").ok().filter(|u| !u.trim().is_empty()).map(|u| u.trim().trim_end_matches('/').to_string())
}

/// The running server's URL (`http://127.0.0.1:<port>/v1`), starting it if
/// it isn't up.
pub async fn ensure() -> Result<String, String> {
    #[cfg(debug_assertions)]
    if let Some(url) = eval_server() {
        return Ok(url);
    }
    if crate::shell::e2e::active() {
        #[cfg(debug_assertions)]
        if let Some(url) = stand_in() {
            return Ok(url);
        }
        return Err("Test runs never start the search model".to_string());
    }
    if let Some(url) = alive() {
        return Ok(url);
    }
    let _one = STARTING.lock().await;
    if let Some(url) = alive() {
        return Ok(url);
    }
    if CLOSING.load(Ordering::SeqCst) {
        return Err("Yap is closing".to_string());
    }
    if crate::ai::local_llm::restart_under_way() {
        return Err("The on-device AI engine is being updated; try again in a moment".to_string());
    }
    if FAILED_STARTS.load(Ordering::SeqCst) >= MAX_FAILED_STARTS {
        return Err("The search model didn't start on this PC".to_string());
    }
    if !installed() {
        return Err("The search model isn't downloaded".to_string());
    }
    match start().await {
        Ok(url) => {
            FAILED_STARTS.store(0, Ordering::SeqCst);
            Ok(url)
        }
        Err(e) => {
            let n = FAILED_STARTS.fetch_add(1, Ordering::SeqCst) + 1;
            tracing::warn!(tries = n, "Meeting search: the search model didn't start: {e}");
            Err(e)
        }
    }
}

/// The server's URL while it runs (one that exited is forgotten).
fn alive() -> Option<String> {
    let mut g = RUNNING.lock().unwrap_or_else(|p| p.into_inner());
    let r = g.as_mut()?;
    let gone = !matches!(r.child.try_wait(), Ok(None));
    if gone {
        tracing::info!("Meeting search: the search model's server had exited");
        *g = None;
        return None;
    }
    g.as_ref().map(|r| r.url.clone())
}

async fn start() -> Result<String, String> {
    let exe = crate::ai::local_llm::runtime_path();
    let model = model_path();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .map_err(|e| format!("no free port: {e}"))?;
    let log_path = dir().join("embedder.log");
    let log = std::fs::File::create(&log_path).map_err(|e| format!("Couldn't write the search model's log: {e}"))?;
    let mut cmd = std::process::Command::new(&exe);
    cmd.args(server_args(&model, port, threads()))
        .current_dir(dir())
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console, and below normal priority: a dictation, a call and
        // whatever the person is doing come first.
        cmd.creation_flags(0x0800_0000 | 0x0000_4000);
    }
    let t0 = Instant::now();
    let child = cmd.spawn().map_err(|e| format!("couldn't start the search model: {e}"))?;
    crate::shell::procs::end_with_yap(&child);
    let url = format!("http://127.0.0.1:{port}/v1");
    *RUNNING.lock().unwrap_or_else(|p| p.into_inner()) = Some(Running { child, url: url.clone() });
    LAST_USE.store(now_ms(), Ordering::SeqCst);
    watch_idle();
    let client = reqwest::Client::builder().timeout(Duration::from_secs(3)).build().map_err(|e| e.to_string())?;
    let health = format!("http://127.0.0.1:{port}/health");
    while t0.elapsed() < START_WAIT {
        if CLOSING.load(Ordering::SeqCst) {
            stop();
            return Err("Yap is closing".to_string());
        }
        if alive().is_none() {
            return Err("the search model's server exited while loading".to_string());
        }
        if client.get(&health).send_for(crate::privacy::net::Purpose::Embeddings).await.is_ok_and(|r| r.status().is_success()) {
            let text = std::fs::read_to_string(&log_path).unwrap_or_default();
            let on = crate::ai::local_llm::placement_after_load(&text).map(|p| p.kind()).unwrap_or("cpu");
            if let Ok(mut g) = ON.lock() {
                *g = Some(on);
            }
            tracing::info!(port, on, ms = t0.elapsed().as_millis() as u64, "Meeting search: the search model is up");
            return Ok(url);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    stop();
    Err("the search model wasn't ready in time".to_string())
}

/// Stops the server once it's gone [`IDLE`] unused (one watcher, for good).
fn watch_idle() {
    if WATCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async {
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            let idle = now_ms().saturating_sub(LAST_USE.load(Ordering::SeqCst)) > IDLE.as_millis() as u64;
            if idle && IN_FLIGHT.load(Ordering::SeqCst) == 0 && RUNNING.lock().is_ok_and(|g| g.is_some()) {
                tracing::info!("Meeting search: the search model went unused; stopping it");
                stop();
            }
        }
    });
}

/// Stop the server (safe when it isn't running).
pub fn stop() {
    let mut g = RUNNING.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(mut r) = g.take() {
        let _ = r.child.kill();
        let _ = r.child.wait();
        tracing::info!("Meeting search: the search model stopped");
    }
}

/// Try starting it again (Rebuild) after starts that failed.
pub fn reset_failures() {
    FAILED_STARTS.store(0, Ordering::SeqCst);
}

/// Yap is quitting.
pub fn shutdown() {
    CLOSING.store(true, Ordering::SeqCst);
    stop();
}

/// A request to the server under way, until dropped.
struct Busy;

impl Busy {
    fn new() -> Busy {
        IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
        Busy
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
        LAST_USE.store(now_ms(), Ordering::SeqCst);
    }
}

/// The embeddings of `texts` (each with its prompt already, [`document`] /
/// [`query`]): unit length, [`DIMS`] each, in order.
pub async fn embed(texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let url = ensure().await?;
    let _busy = Busy::new();
    LAST_USE.store(now_ms(), Ordering::SeqCst);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60 + 10 * texts.len() as u64))
        .build()
        .map_err(|e| e.to_string())?;
    let body = serde_json::json!({ "model": MODEL_TAG, "input": texts });
    let res = client
        .post(format!("{url}/embeddings"))
        .json(&body)
        .send_for(crate::privacy::net::Purpose::Embeddings)
        .await
        .map_err(|e| format!("the search model didn't answer: {e}"))?;
    let status = res.status();
    let v: serde_json::Value = res.json().await.map_err(|e| format!("the search model's answer didn't read: {e}"))?;
    if !status.is_success() {
        // Never the texts: only what the server says went wrong.
        let msg = v["error"]["message"].as_str().unwrap_or("").chars().take(160).collect::<String>();
        return Err(format!("the search model said HTTP {}{}", status.as_u16(), if msg.is_empty() { String::new() } else { format!(": {msg}") }));
    }
    parse(&v, texts.len())
}

/// The vectors in an `/embeddings` answer, in input order, unit length and
/// [`DIMS`] long (a longer one cut — Matryoshka — a shorter one refused).
/// Pure.
pub fn parse(v: &serde_json::Value, want: usize) -> Result<Vec<Vec<f32>>, String> {
    let data = v["data"].as_array().ok_or("no vectors in the search model's answer")?;
    let mut rows: Vec<(u64, Vec<f32>)> = data
        .iter()
        .map(|d| {
            let e: Vec<f32> = d["embedding"].as_array().into_iter().flatten().filter_map(|x| x.as_f64().map(|f| f as f32)).collect();
            (d["index"].as_u64().unwrap_or(0), e)
        })
        .collect();
    if rows.len() != want {
        return Err(format!("the search model answered {} vectors for {want} passages", rows.len()));
    }
    rows.sort_by_key(|(i, _)| *i);
    rows.into_iter()
        .map(|(_, e)| {
            if e.len() < DIMS {
                return Err(format!("the search model's vectors have {} dimensions, not {DIMS}", e.len()));
            }
            Ok(super::store::normalised(&e[..DIMS]))
        })
        .collect()
}

/// One question's vector.
pub async fn embed_query(q: &str) -> Result<Vec<f32>, String> {
    embed(&[query(q)]).await?.pop().ok_or_else(|| "no vector for the question".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_model_is_pinned() {
        assert!(MODEL_URL.starts_with("https://huggingface.co/ggml-org/embeddinggemma-300M-GGUF/resolve/"));
        assert!(MODEL_URL.ends_with(MODEL_FILE));
        assert_eq!(MODEL_SHA256.len(), 64);
        assert!(MODEL_SHA256.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn prompts_as_the_model_card_has_them() {
        assert_eq!(document("Launch review", "Them: March third."), "title: Launch review | text: Them: March third.");
        assert_eq!(document(" ", "x"), "title: none | text: x");
        assert_eq!(query(" when is the launch? "), "task: search result | query: when is the launch?");
    }

    #[test]
    fn the_server_reads_passages_on_the_card_below_the_dictation() {
        let args = server_args(Path::new("m.gguf"), 5001, 4);
        let after = |flag: &str| args.iter().position(|a| a == flag).map(|i| args[i + 1].as_str());
        assert!(args.iter().any(|a| a == "--embedding") && args.iter().any(|a| a == "--server"));
        assert_eq!(after("--host"), Some("127.0.0.1"), "this PC only");
        assert_eq!(after("--port"), Some("5001"));
        assert_eq!(after("-ngl"), Some("999"));
        assert_eq!(after("-t"), Some("4"));
        assert_eq!(after("-ub"), Some("2048"), "a whole passage in one batch");
        assert!(threads() >= 2);
    }

    #[test]
    fn answers_are_read_in_order_and_cut_to_size() {
        let v = |n: usize, x: f32| json!(vec![x; n]);
        let answer = json!({ "data": [
            { "index": 1, "embedding": v(DIMS + 256, 2.0) },
            { "index": 0, "embedding": v(DIMS, 1.0) },
        ]});
        let rows = parse(&answer, 2).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.len() == DIMS));
        let norm: f32 = rows[0].iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "unit length");
        assert!(parse(&answer, 3).is_err(), "one missing");
        assert!(parse(&json!({ "data": [{ "index": 0, "embedding": [1.0, 2.0] }] }), 1).is_err(), "too short");
        assert!(parse(&json!({ "error": "x" }), 1).is_err());
    }

    /// The real thing, once: the pinned runtime serving the pinned model
    /// (`YAP_SEARCH_LLAMAFILE` and `YAP_SEARCH_MODEL`: their paths), a
    /// passage and two questions — the one it answers closer.
    /// `cargo test real_embeddings -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_embeddings_put_the_right_passage_closest() {
        let (Some(exe), Some(model)) = (std::env::var_os("YAP_SEARCH_LLAMAFILE"), std::env::var_os("YAP_SEARCH_MODEL")) else {
            eprintln!("set YAP_SEARCH_LLAMAFILE and YAP_SEARCH_MODEL");
            return;
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
            let mut child = std::process::Command::new(exe)
                .args(server_args(Path::new(&model), port, threads()))
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            let client = reqwest::Client::new();
            let mut up = false;
            for _ in 0..240 {
                if client.get(format!("http://127.0.0.1:{port}/health")).send_for(crate::privacy::net::Purpose::Embeddings).await.is_ok_and(|r| r.status().is_success()) {
                    up = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            assert!(up, "the server came up");
            let texts = vec![
                document("Dentist", "Appointment with Dr Patel on Thursday at 9:40 for a scale and polish."),
                query("what time is my teeth cleaning"),
                query("when does the beta ship"),
            ];
            let t0 = Instant::now();
            let res: serde_json::Value = client
                .post(format!("http://127.0.0.1:{port}/v1/embeddings"))
                .json(&json!({ "model": MODEL_TAG, "input": texts }))
                .send_for(crate::privacy::net::Purpose::Embeddings)
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            let rows = parse(&res, 3).unwrap();
            let near = super::super::rank::cosine(&rows[0], &rows[1]);
            let far = super::super::rank::cosine(&rows[0], &rows[2]);
            eprintln!("3 texts in {} ms; dentist·teeth {near:.3}, dentist·beta {far:.3}", t0.elapsed().as_millis());
            let _ = child.kill();
            let _ = child.wait();
            assert!(near > far + 0.1);
        });
    }
}

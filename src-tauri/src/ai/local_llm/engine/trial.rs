//! **The trial**: a build tried with the person's own model, out of sight,
//! before Yap switches to it — and the engine it would replace tried the
//! same way, one after the other, so the two are compared on the same PC
//! at the same moment. Each is started as the sidecar would start it
//! (`local_llm::server_args`, the model's context, MTP and KV cache as the
//! sidecar's plan says) on a free port, its log in `engine/trial.log`, and
//! asked, with made-up words only:
//! - to answer ("Reply with the single word: ready");
//! - to call a tool, when its chat format takes tools (`/props`);
//! - for ~128 tokens, twice: its generation speed (llama.cpp's own
//!   `timings.predicted_per_second`, the better of the two);
//! - with the projector, when one is installed and on ("Read screenshots on
//!   this PC"): it loads, and a tiny picture gets an answer.
//!
//! [`verdict`] compares them: the new build must start, answer, make a
//! valid tool call and read a picture wherever the current one does, and
//! generate at [`MIN_SPEED`] of its speed or better. A trial gives way the
//! moment the person needs the model (`abort`): its server is ended and the
//! trial tried again later. Every request is to this PC
//! ("On-device AI · trying a new engine" in Network connections).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::{Engine, Variant, Why};
use crate::privacy::net::Recorded as _;

/// The new build passes at this share of the current engine's generation
/// speed or better: relative to the engine running, so a real regression
/// is caught, with room for the few percent builds and runs differ by (the
/// better of two runs is kept; b11982 against llamafile 0.10.6 measured
/// within 3 %).
pub const MIN_SPEED: f64 = 0.85;

/// Generation the maintainer called fine for the 4B on an RTX 5070 Ti
/// (2026-11-01, "about 100 tok/s is fine"): when the CUDA build won't
/// start or pass here, a move from CUDA (llamafile's, or a CUDA build's) to
/// the Vulkan build passes at it even when relatively slower ([`verdict`]):
/// Vulkan is the fallback.
pub const GOOD_ENOUGH_TPS: f64 = 100.0;
/// Generation tokens a speed run asks for.
const SPEED_TOKENS: u32 = 128;
/// How long a build may take to load (a first run under an antivirus).
const LOAD_WAIT: Duration = Duration::from_secs(300);

/// What a trial found.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Trial {
    pub started: bool,
    pub answered: bool,
    /// A valid tool call (`None`: its chat format takes no tools, or not
    /// asked).
    pub tool: Option<bool>,
    /// Generation speed, tokens/s.
    pub tps: Option<f64>,
    /// The projector loaded and a picture was answered (`None`: not tried).
    pub vision: Option<bool>,
    pub load_ms: u64,
    /// It gave way to the person (not a verdict on the build).
    pub aborted: bool,
    /// It didn't start, and its log blames the model ([`super::log_blames_model`]):
    /// not a verdict on the build either.
    #[serde(default)]
    pub model_failed: bool,
}

/// A trial's outcome for the new build.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verdict {
    /// Switch to it; `ratio` = its speed against the current engine's.
    Pass { ratio: Option<f64> },
    Fail { why: Why, ratio: Option<f64> },
}

/// **The new build against the current engine** (`None`: nothing to
/// compare with). A check the current engine fails too isn't held against
/// the new one. `to_vulkan`: the current engine runs on CUDA and the new
/// build is Vulkan — the simpler build chosen on purpose — so
/// [`GOOD_ENOUGH_TPS`] passes too. Pure.
pub fn verdict(current: Option<&Trial>, new: &Trial, to_vulkan: bool) -> Verdict {
    let ratio = match (new.tps, current.and_then(|c| c.tps)) {
        (Some(n), Some(c)) if c > 0.0 => Some(n / c),
        _ => None,
    };
    let fail = |why| Verdict::Fail { why, ratio };
    let current_also = |check: fn(&Trial) -> Option<bool>| current.is_some_and(|c| check(c) == Some(false));
    if !new.started {
        return fail(Why::DidntStart);
    }
    if !new.answered {
        return fail(Why::NoAnswer);
    }
    if new.tool == Some(false) && !current_also(|t| t.tool) {
        return fail(Why::ToolCall);
    }
    let good_enough = to_vulkan && new.tps.is_some_and(|t| t >= GOOD_ENOUGH_TPS);
    if ratio.is_some_and(|r| r < MIN_SPEED) && !good_enough {
        return fail(Why::Slower);
    }
    if new.vision == Some(false) && !current_also(|t| t.vision) {
        return fail(Why::Vision);
    }
    Verdict::Pass { ratio }
}

/// **Which of one build's variants that passed** (with their generation
/// speeds): CUDA when it's at least as fast as Vulkan (on an NVIDIA card it
/// is: 269 against 150 tokens/s for the 4B on an RTX 5070 Ti, and a
/// dictation's cleanup 132 against 200 ms); any other set, the fastest.
/// Pure.
pub fn choose(passed: &[(Variant, f64)]) -> Option<Variant> {
    let vulkan = passed.iter().find(|(v, _)| *v == Variant::Vulkan).map(|(_, t)| *t);
    let cuda = passed.iter().filter(|(v, _)| v.cuda()).max_by(|a, b| a.1.total_cmp(&b.1)).copied();
    match (vulkan, cuda) {
        (Some(vk), Some((c, ct))) => Some(if ct >= vk { c } else { Variant::Vulkan }),
        _ => passed.iter().max_by(|a, b| a.1.total_cmp(&b.1)).map(|(v, _)| *v),
    }
}

/// A build the floor falls back to, with nothing running to compare it
/// with (`updater::rescue`): it started and answered. Its tools and the
/// projector are the model's, not the engine's, with no other to compare.
/// Pure.
pub fn rescue_ok(t: &Trial) -> bool {
    t.started && t.answered && !t.aborted
}

/// A tool-call answer: the tool named, its arguments JSON with a city.
/// Pure.
pub fn valid_tool_call(message: &serde_json::Value) -> bool {
    let Some(call) = message["tool_calls"].as_array().and_then(|c| c.first()) else { return false };
    if call["function"]["name"].as_str() != Some("get_weather") {
        return false;
    }
    let args = &call["function"]["arguments"];
    let parsed = match args {
        serde_json::Value::String(s) => serde_json::from_str::<serde_json::Value>(s).ok(),
        v @ serde_json::Value::Object(_) => Some(v.clone()),
        _ => None,
    };
    parsed.is_some_and(|a| a["city"].as_str().is_some_and(|c| !c.trim().is_empty()))
}

/// A 64 × 64 red PNG (made here; nobody's picture).
fn tiny_png() -> String {
    use base64::Engine as _;
    let rgba: Vec<u8> = std::iter::repeat_n([220u8, 30, 30, 255], 64 * 64).flatten().collect();
    let bytes = crate::snip::images::encode_png(&rgba, 64, 64, true).unwrap_or_default();
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// The server a trial runs, ended when dropped.
struct Server {
    child: std::process::Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// What gave way: the person needed the model.
struct Aborted;

/// Wait for `fut`, giving way when `abort()` says so (the request dropped:
/// llama.cpp stops it).
async fn or_abort<T>(fut: impl std::future::Future<Output = T>, abort: &(dyn Fn() -> bool + Sync)) -> Result<T, Aborted> {
    let watch = async {
        loop {
            if abort() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    tokio::select! {
        v = fut => Ok(v),
        _ = watch => Err(Aborted),
    }
}

/// Start `engine` serving `model` as the sidecar would (+ `projector`).
async fn start(
    engine: &Engine,
    model: &Path,
    ctx: u32,
    plan: crate::ai::local_llm::Plan,
    projector: Option<&crate::ai::local_llm::vision::Load>,
    log: &Path,
    abort: &(dyn Fn() -> bool + Sync),
) -> Result<Option<(Server, u64)>, Aborted> {
    let Some(port) = std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map(|a| a.port()).ok() else {
        return Ok(None);
    };
    let Ok(file) = std::fs::File::create(log) else { return Ok(None) };
    let mut args = crate::ai::local_llm::server_args(model, port, 999, ctx, plan);
    if let Some(p) = projector {
        args.extend(crate::ai::local_llm::vision::args(p));
    }
    let mut cmd = engine.command();
    cmd.args(args).stdin(std::process::Stdio::null());
    if let Ok(f) = file.try_clone() {
        cmd.stdout(f);
    }
    cmd.stderr(file);
    let t0 = Instant::now();
    let Ok(child) = cmd.spawn() else { return Ok(None) };
    crate::shell::procs::end_with_yap(&child);
    let mut server = Server { child, port };
    let client = reqwest::Client::builder().timeout(Duration::from_secs(3)).build().map_err(|_| Aborted)?;
    let health = format!("http://127.0.0.1:{port}/health");
    while t0.elapsed() < LOAD_WAIT {
        if abort() {
            return Err(Aborted);
        }
        if matches!(server.child.try_wait(), Ok(Some(_))) {
            return Ok(None);
        }
        if client.get(&health).send_for(crate::privacy::net::Purpose::EngineTrial).await.is_ok_and(|r| r.status().is_success()) {
            return Ok(Some((server, t0.elapsed().as_millis() as u64)));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    // Not ready in time: ended as it drops.
    Ok(None)
}

/// One chat request to the trial's server; its JSON answer.
async fn ask(client: &reqwest::Client, port: u16, body: serde_json::Value) -> Option<serde_json::Value> {
    let resp = client
        .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .json(&body)
        .send_for(crate::privacy::net::Purpose::EngineTrial)
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json().await.ok()
}

/// **Try `engine`** with `model` at `ctx` (and `projector`, when the person
/// has one on), as the sidecar would run it. Never logs a word of any
/// answer.
pub async fn run(engine: &Engine, model: &Path, ctx: u32, vision_on: bool, log: &Path, abort: &(dyn Fn() -> bool + Sync)) -> Trial {
    match run_inner(engine, model, ctx, vision_on, log, abort).await {
        Ok(t) => t,
        Err(Aborted) => Trial { aborted: true, ..Default::default() },
    }
}

async fn run_inner(engine: &Engine, model: &Path, ctx: u32, vision_on: bool, log: &Path, abort: &(dyn Fn() -> bool + Sync)) -> Result<Trial, Aborted> {
    let cfg = crate::config::load();
    let plan = crate::ai::local_llm::plan_for(&cfg, model, ctx, engine.runtime());
    let mut t = Trial::default();
    let Some((server, load_ms)) = start(engine, model, ctx, plan, None, log, abort).await? else {
        t.model_failed = super::log_blames_model(&std::fs::read_to_string(log).unwrap_or_default());
        return Ok(t);
    };
    t.started = true;
    t.load_ms = load_ms;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(180)).build().map_err(|_| Aborted)?;
    let id = crate::ai::local_llm::model_id(model);
    let port = server.port;

    // It answers.
    let body = serde_json::json!({
        "model": id, "temperature": 0, "max_tokens": 16,
        "messages": [{"role": "user", "content": "Reply with the single word: ready"}],
    });
    let answer = or_abort(ask(&client, port, body), abort).await?;
    t.answered = answer
        .as_ref()
        .and_then(|a| a["choices"][0]["message"]["content"].as_str())
        .is_some_and(|c| !c.trim().is_empty());
    if !t.answered {
        return Ok(t);
    }

    // A tool call, when its chat format takes tools.
    let props: Option<serde_json::Value> = or_abort(
        async {
            let r = client.get(format!("http://127.0.0.1:{port}/props")).send_for(crate::privacy::net::Purpose::EngineTrial).await.ok()?;
            r.json().await.ok()
        },
        abort,
    )
    .await?;
    if props.as_ref().and_then(crate::ai::local_llm::template_caps_take_tools).unwrap_or(true) {
        let body = serde_json::json!({
            "model": id, "temperature": 0, "max_tokens": 160,
            "messages": [{"role": "user", "content": "What's the weather in Paris right now? Use the tool."}],
            "tools": [{"type": "function", "function": {
                "name": "get_weather", "description": "The weather in a city now",
                "parameters": {"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]},
            }}],
        });
        let a = or_abort(ask(&client, port, body), abort).await?;
        t.tool = Some(a.as_ref().is_some_and(|a| valid_tool_call(&a["choices"][0]["message"])));
    }

    // Its generation speed: the better of two runs.
    let mut best: Option<f64> = None;
    for _ in 0..2 {
        let body = serde_json::json!({
            "model": id, "temperature": 0, "max_tokens": SPEED_TOKENS,
            "messages": [{"role": "user", "content": "Write about two hundred words on why the sea is salty."}],
        });
        let a = or_abort(ask(&client, port, body), abort).await?;
        let tps = a.as_ref().and_then(|a| {
            let n = a["timings"]["predicted_n"].as_u64().unwrap_or(0);
            (n >= 16).then(|| a["timings"]["predicted_per_second"].as_f64()).flatten()
        });
        if let Some(v) = tps {
            best = Some(best.map_or(v, |b: f64| b.max(v)));
        }
    }
    t.tps = best;
    drop(server);

    // The projector, when the person has one on for this model.
    let projector = vision_on.then(|| crate::ai::local_llm::vision::for_trial(&cfg, model, ctx, plan)).flatten();
    if let Some(p) = projector {
        let vision_log: PathBuf = log.with_file_name("trial-vision.log");
        t.vision = Some(false);
        if let Some((server, _)) = start(engine, model, ctx, plan, Some(&p), &vision_log, abort).await? {
            let loaded = crate::ai::local_llm::vision::in_log(&std::fs::read_to_string(&vision_log).unwrap_or_default()).is_some();
            let body = serde_json::json!({
                "model": id, "temperature": 0, "max_tokens": 16,
                "messages": [{"role": "user", "content": [
                    {"type": "text", "text": "What colour is this picture? One word."},
                    {"type": "image_url", "image_url": {"url": tiny_png()}},
                ]}],
            });
            let a = or_abort(ask(&client, server.port, body), abort).await?;
            let answered = a.as_ref().and_then(|a| a["choices"][0]["message"]["content"].as_str()).is_some_and(|c| !c.trim().is_empty());
            t.vision = Some(loaded && answered);
        }
    }
    Ok(t)
}

/// Test mode (debug builds): a scripted trial, nothing run —
/// `YAP_E2E_ENGINE_TRIAL` = `{"b11980": {"tps": 180}, "b11990": {"fail":
/// "start"}, "current": {"tps": 170}}` ("start" | "answer" | "tool" |
/// "vision" | "model"), keyed by build ("b11980") or, for the floor's
/// fallback, by build and variant ("b11982-cuda12"); any other passes at
/// 100 tokens/s.
#[cfg(debug_assertions)]
pub fn scripted(key: &str) -> Trial {
    let spec: serde_json::Value = std::env::var("YAP_E2E_ENGINE_TRIAL").ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let s = &spec[key];
    let pass = Trial { started: true, answered: true, tool: Some(true), tps: Some(s["tps"].as_f64().unwrap_or(100.0)), vision: None, load_ms: 10, aborted: false, model_failed: false };
    match s["fail"].as_str() {
        Some("start") => Trial::default(),
        Some("model") => Trial { model_failed: true, ..Trial::default() },
        Some("answer") => Trial { answered: false, tool: None, tps: None, ..pass },
        Some("tool") => Trial { tool: Some(false), ..pass },
        Some("vision") => Trial { vision: Some(false), ..pass },
        _ => pass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(tps: f64) -> Trial {
        Trial { started: true, answered: true, tool: Some(true), tps: Some(tps), vision: None, load_ms: 1, aborted: false, model_failed: false }
    }

    #[test]
    fn a_new_build_must_do_all_the_current_one_does_at_nearly_its_speed() {
        let cur = ok(170.0);
        let v = |c: &Trial, n: &Trial| verdict(Some(c), n, false);
        assert_eq!(v(&cur, &ok(176.0)), Verdict::Pass { ratio: Some(176.0 / 170.0) });
        assert!(matches!(v(&cur, &ok(150.0)), Verdict::Pass { .. }), "88%: within the bar");
        assert!(matches!(v(&cur, &ok(165.0)), Verdict::Pass { .. }), "build-to-build noise");
        assert_eq!(v(&cur, &ok(136.0)), Verdict::Fail { why: Why::Slower, ratio: Some(0.8) });
        assert!(matches!(v(&cur, &Trial::default()), Verdict::Fail { why: Why::DidntStart, .. }));
        assert!(matches!(v(&cur, &Trial { answered: false, ..ok(170.0) }), Verdict::Fail { why: Why::NoAnswer, .. }));
        assert!(matches!(v(&cur, &Trial { tool: Some(false), ..ok(170.0) }), Verdict::Fail { why: Why::ToolCall, .. }));
        // A model whose chat format takes no tools: nothing to hold against it.
        assert!(matches!(v(&Trial { tool: None, ..cur.clone() }, &Trial { tool: None, ..ok(170.0) }), Verdict::Pass { .. }));
        // The current one can't call the tool either (a small model): not the build's fault.
        assert!(matches!(v(&Trial { tool: Some(false), ..cur.clone() }, &Trial { tool: Some(false), ..ok(170.0) }), Verdict::Pass { .. }));
        // Screenshots: only when the current one reads them.
        let seeing = Trial { vision: Some(true), ..cur.clone() };
        assert!(matches!(v(&seeing, &Trial { vision: Some(false), ..ok(170.0) }), Verdict::Fail { why: Why::Vision, .. }));
        assert!(matches!(v(&Trial { vision: Some(false), ..cur.clone() }, &Trial { vision: Some(false), ..ok(170.0) }), Verdict::Pass { .. }));
        // No speed to compare (the current one didn't say): it passes on the rest.
        assert_eq!(v(&Trial { tps: None, ..cur.clone() }, &ok(100.0)), Verdict::Pass { ratio: None });
        assert_eq!(verdict(None, &ok(100.0), false), Verdict::Pass { ratio: None });
    }

    #[test]
    fn cuda_is_kept_when_at_least_as_fast_and_vulkan_is_the_fallback() {
        // CUDA against Vulkan on an NVIDIA card: CUDA when it's at least as fast.
        assert_eq!(choose(&[(Variant::Cuda13, 269.0), (Variant::Vulkan, 150.0)]), Some(Variant::Cuda13), "the RTX 5070 Ti's numbers");
        assert_eq!(choose(&[(Variant::Cuda13, 150.0), (Variant::Vulkan, 150.0)]), Some(Variant::Cuda13), "as fast: CUDA");
        assert_eq!(choose(&[(Variant::Cuda12, 90.0), (Variant::Vulkan, 120.0)]), Some(Variant::Vulkan), "an odd card where Vulkan is faster");
        assert_eq!(choose(&[(Variant::Vulkan, 152.0)]), Some(Variant::Vulkan), "CUDA didn't pass: Vulkan");
        assert_eq!(choose(&[(Variant::Cuda12, 40.0)]), Some(Variant::Cuda12));
        assert_eq!(choose(&[(Variant::Cpu, 25.0)]), Some(Variant::Cpu));
        assert_eq!(choose(&[]), None);
        // When CUDA won't pass, a move from CUDA to Vulkan passes at ~100 tokens/s.
        let cuda = ok(225.0);
        assert!(matches!(verdict(Some(&cuda), &ok(150.0), true), Verdict::Pass { .. }), "67% but over 100 tokens/s");
        assert!(matches!(verdict(Some(&cuda), &ok(150.0), false), Verdict::Fail { why: Why::Slower, .. }), "the same move inside a build family: relative");
        assert!(matches!(verdict(Some(&cuda), &ok(90.0), true), Verdict::Fail { why: Why::Slower, .. }), "under 100 and much slower");
        assert!(matches!(verdict(Some(&ok(100.0)), &ok(90.0), true), Verdict::Pass { .. }), "under 100 but within the relative bar");
        // The floor's fallback: started and answered is enough (no engine to compare with).
        assert!(rescue_ok(&ok(20.0)));
        assert!(rescue_ok(&Trial { tool: Some(false), ..ok(20.0) }), "tools are the model's");
        assert!(!rescue_ok(&Trial::default()));
        assert!(!rescue_ok(&Trial { answered: false, ..ok(20.0) }));
        assert!(!rescue_ok(&Trial { aborted: true, ..ok(20.0) }));
    }

    /// The real thing: a trial of a llama.cpp build on this PC
    /// (`YAP_TRIAL_ENGINE`: its folder; `YAP_TRIAL_VARIANT`: cuda13 | vulkan |
    /// cpu; `YAP_TRIAL_CUDART`: NVIDIA's runtime folder for CUDA;
    /// `YAP_TRIAL_MODEL`: a GGUF, read in place; `YAP_TRIAL_DATA`: a scratch
    /// data folder — with `llm/vision/<projector>` and `config.json`
    /// `{"localVision": true}` in it the projector is tried too). Never the
    /// person's own data folder.
    /// `cargo test real_trial -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn real_trial_of_a_build() {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let (Some(dir), Some(model), Some(data)) = (var("YAP_TRIAL_ENGINE"), var("YAP_TRIAL_MODEL"), var("YAP_TRIAL_DATA")) else {
            eprintln!("set YAP_TRIAL_ENGINE, YAP_TRIAL_MODEL and YAP_TRIAL_DATA");
            return;
        };
        assert!(crate::shell::portable::use_data_dir(std::path::PathBuf::from(&data)), "a scratch data folder");
        let variant = var("YAP_TRIAL_VARIANT").and_then(|v| Variant::from_id(&v)).unwrap_or(Variant::Vulkan);
        let installed = super::super::Installed { build: super::super::FLOOR, variant, sha256: String::new(), cudart: None, device: var("YAP_TRIAL_DEVICE") };
        let engine = Engine::LlamaCpp { dir: dir.into(), installed, cudart: var("YAP_TRIAL_CUDART").map(Into::into) };
        let log = std::path::PathBuf::from(&data).join("trial.log");
        let cfg = crate::config::load();
        let t0 = std::time::Instant::now();
        let t = run(&engine, std::path::Path::new(&model), 8_192, cfg.local_vision, &log, &|| false).await;
        println!("trial in {:.1} s: {t:?}", t0.elapsed().as_secs_f64());
        assert!(t.started && t.answered && t.tool == Some(true) && t.tps.is_some_and(|v| v > 1.0), "{t:?}");
        if cfg.local_vision {
            assert_eq!(t.vision, Some(true), "the projector loaded and answered");
        }
    }

    #[test]
    fn a_tool_call_must_name_the_tool_with_a_city() {
        let call = |name: &str, args: serde_json::Value| serde_json::json!({"tool_calls": [{"type": "function", "function": {"name": name, "arguments": args}}]});
        assert!(valid_tool_call(&call("get_weather", serde_json::json!("{\"city\":\"Paris\"}"))));
        assert!(valid_tool_call(&call("get_weather", serde_json::json!({"city": "Paris"}))));
        assert!(!valid_tool_call(&call("get_weather", serde_json::json!("{\"city\":\"\"}"))));
        assert!(!valid_tool_call(&call("get_weather", serde_json::json!("not json"))));
        assert!(!valid_tool_call(&call("get_time", serde_json::json!("{\"city\":\"Paris\"}"))));
        assert!(!valid_tool_call(&serde_json::json!({"content": "It's sunny in Paris."})));
        assert!(tiny_png().starts_with("data:image/png;base64,"));
    }
}

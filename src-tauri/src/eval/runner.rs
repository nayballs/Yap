//! The harness: arguments, keys, models (a local one's llamafile started and
//! stopped), scenarios run through `commands::chat_turn`, scoring, and the
//! run folder (`transcript.jsonl`, `summary.md`, `summary.json`, `yap.log`,
//! llamafile logs, `spike.md`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::local;
use super::record;
use super::scenario::{self, AppEntry, Apps, Observed, Scenario};
use super::standin::Standin;

/// Chat's persona as Settings sets it by default (`Settings.svelte`, the
/// Chat scope's prompt), which `chat_turn` builds on.
pub(super) const DEFAULT_PERSONA: &str = "You are a helpful voice assistant. Answer concisely and conversationally, and handle informal spoken phrasing gracefully. Keep answers brief unless asked for detail.";

const USAGE: &str = "\
yap.exe eval [options]          (debug builds; docs/model-evals.md)

Models (repeat --model to compare):
  --model groq:openai/gpt-oss-20b     --model openai:<model>     --model anthropic:<model>
  --model openrouter:<model>          --model gemini:<model>     --model custom:<base_url>,<model>
  --model local:installed             (Yap's on-device Qwen2.5-1.5B; read-only from %APPDATA%\\yap\\llm)
  --model local:<curated id>          (local_llm.rs' list, e.g. qwen2.5-3b, llama-3.2-3b)
  --model local:<path.gguf>           --model local:<https url>#sha256=<hex>

Keys: GROQ_API_KEY, OPENAI_API_KEY, ANTHROPIC_API_KEY, OPENROUTER_API_KEY, GEMINI_API_KEY,
      YAP_EVAL_CUSTOM_KEY, EXA_API_KEY, TAVILY_API_KEY; or --keys-from-yap (the keys saved in
      %APPDATA%\\yap\\config.json, web search's too, read-only). Keys are never printed or
      written; every log line is redacted.

Scenarios:
  --scenarios <dir|file>   (default evals/scenarios)   --only <name,...>   --tag <tag,...>
  --no-web                 skip the web scenarios (needs_web / web): live ones ask the real
                           search service (the run's Exa/Tavily key) or Wikipedia and game wikis
  --repeat <n>             run each scenario n times     --list   list the scenarios
Running:
  --tools on|off           treat the model as able (or not) to call tools (default: Yap's rule)
  --tokens-per-minute <n>  pacing budget (default 8000 for Groq, its free plan; none otherwise)
  --max-total-tokens <n>   stop starting scenarios for a model past this many tokens
  --timeout <secs>         per model call (default: Yap's own, 20 s for Chat)
  --clock-step <minutes>   each turn the clock Chat tells the model moves on this much (a real
                           chat's turns are minutes apart; default 0: the real time)
  --locale <tag|json>      the user's region, as Windows would say it (About you, about_you.rs):
                           \"en-GB\" (currency and units from its region), or JSON {locale, country,
                           currency, symbol, metric, timeZone}; default none (no region in prompts)
Local models (Yap's llamafile, like local_llm.rs, on a free port, stopped after):
  --ngl <n>                GPU layers (default 0: CPU only)   --ctx <n> (default 8192, as Yap)
  --llamafile <path>       the runtime (default: the installed one, else downloaded)
  --llamafile-arg <arg>    extra server argument (repeatable)
  --mtp on|off             MTP speculative decoding (default: as Yap's sidecar decides on this PC;
                           never with --ngl 0)
  --no-warm-up             skip the warm-up request Yap's sidecar sends once the model is loaded
  --models-dir <dir>       downloads go here (default <out>/../eval-models)
Output:
  --out <dir>              run folders go here (default $YAP_EVAL_OUT, else test-results/evals)
  --label <text>           names the run folder
Raw requests:
  --raw <file.json>        send hand-written requests (e.g. evals/raw/groq-browser-search.json),
                           logged like the rest, summarised in spike.md
Dictation cleanup:
  --cleanup                instead of the scenarios: time llm::cleanup on each model over
                           evals/cleanup/dictations.json, back to back and after a Chat turn;
                           cleanup.md   --cleanup-rounds <n> (default 3)
  --cleanup-slot <n>       send the cleanup to llama.cpp slot n (`id_slot`), and with
  --chat-slot <n>          the Chat turns between them to slot n (measures pinned slots)
  --cleanup-layout <l>     the same cleanup prompt laid out another way: combined (the restated
                           instruction and the transcript in one message, as until 22 Oct 2026)
                           | system (the instruction in the system prompt)
  --cleanup-quick          only the back-to-back rounds (no first call, Chat turns or spelling):
                           a cloud model's tokens   --cleanup-limit <n>  only the first n dictations
Speaker names (speakers/naming.rs; instead of the scenarios; speakers.md):
  --speakers               score Rust alone, then Rust + each --model's answers, on
                           evals/speakers/*.json: correct / wrong / missed names per voice
Pre-meeting briefs:
  --brief                  instead of the scenarios: write the brief of each meeting in
                           evals/briefs/*.json (--only picks them) through calendar/brief.rs, and
                           score the model's reply as written and the brief as shown; brief.md
Serving benches (local servers; instead of the scenarios; serving.md):
  --bench stream           time to the first token, streamed vs not, on Chat-shaped requests
                           (a short answer, a long one, a web answer, a tool call)
  --bench layout           a 4-turn Chat with the time + notes in the system prompt (today)
                           vs in the last user message: prompt tokens re-processed per turn
  --bench loop             a web answer's second step after a first that offered other tools
                           (today's: the action tools dropped, web_search only) or the same
  --bench embed            an --embedding server: throughput, query latency, and recall over
                           evals/retrieval/notes.json vs Chat's keyword search
  --bench-rounds <n>       rounds per request (default 3)
  --bench-history <n>      layout: n earlier question/answer pairs (~450 tokens each) first
Requests:
  --temperature <t>        every request's temperature (0: greedy, to compare outputs)
  --set <key>=<json>       a field laid over every request body (repeatable; null removes)
";

#[derive(Debug, Default)]
struct Args {
    models: Vec<String>,
    scenarios: Option<PathBuf>,
    only: Vec<String>,
    tags: Vec<String>,
    no_web: bool,
    keys_from_yap: bool,
    tokens_per_minute: Option<u64>,
    max_total_tokens: Option<u64>,
    timeout: Option<u64>,
    out: Option<PathBuf>,
    label: Option<String>,
    tools: Option<bool>,
    ngl: u32,
    ctx: u32,
    llamafile: Option<PathBuf>,
    llamafile_args: Vec<String>,
    mtp: Option<bool>,
    no_warm_up: bool,
    models_dir: Option<PathBuf>,
    raw: Option<PathBuf>,
    repeat: u32,
    clock_step: u64,
    dump_apps: Option<PathBuf>,
    list: bool,
    cleanup: bool,
    speakers: bool,
    cleanup_rounds: u32,
    locale: Option<String>,
    cleanup_slot: Option<u64>,
    chat_slot: Option<u64>,
    cleanup_layout: Option<&'static str>,
    cleanup_quick: bool,
    cleanup_limit: Option<usize>,
    brief: bool,
    bench: Option<String>,
    bench_rounds: u32,
    bench_history: usize,
    temperature: Option<f64>,
    set: Vec<(String, Value)>,
}

fn parse(argv: Vec<String>) -> Result<Args, String> {
    let mut a = Args { ngl: 0, ctx: 8192, repeat: 1, cleanup_rounds: 3, bench_rounds: 3, ..Default::default() };
    let mut it = argv.into_iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let mut value = || inline.clone().or_else(|| it.next()).ok_or_else(|| format!("{flag} needs a value"));
        let list = |s: String| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>();
        let num = |s: String, f: &str| s.trim().replace('_', "").parse::<u64>().map_err(|_| format!("{f}: not a number: {s}"));
        match flag.as_str() {
            "--model" | "-m" => a.models.push(value()?),
            "--scenarios" => a.scenarios = Some(PathBuf::from(value()?)),
            "--only" => a.only.extend(list(value()?)),
            "--tag" | "--tags" => a.tags.extend(list(value()?)),
            "--no-web" => a.no_web = true,
            "--keys-from-yap" => a.keys_from_yap = true,
            "--tokens-per-minute" | "--tpm" => a.tokens_per_minute = Some(num(value()?, &flag)?),
            "--max-total-tokens" => a.max_total_tokens = Some(num(value()?, &flag)?),
            "--timeout" => a.timeout = Some(num(value()?, &flag)?),
            "--out" => a.out = Some(PathBuf::from(value()?)),
            "--label" => a.label = Some(value()?),
            "--tools" => {
                a.tools = match value()?.as_str() {
                    "on" | "true" | "yes" => Some(true),
                    "off" | "false" | "no" => Some(false),
                    "auto" => None,
                    other => return Err(format!("--tools on|off|auto, not {other}")),
                }
            }
            "--ngl" => a.ngl = num(value()?, &flag)? as u32,
            "--ctx" => a.ctx = num(value()?, &flag)? as u32,
            "--llamafile" => a.llamafile = Some(PathBuf::from(value()?)),
            "--llamafile-arg" => a.llamafile_args.push(value()?),
            "--mtp" => {
                a.mtp = match value()?.as_str() {
                    "on" | "true" | "yes" => Some(true),
                    "off" | "false" | "no" => Some(false),
                    "auto" => None,
                    other => return Err(format!("--mtp on|off|auto, not {other}")),
                }
            }
            "--no-warm-up" => a.no_warm_up = true,
            "--models-dir" => a.models_dir = Some(PathBuf::from(value()?)),
            "--raw" => a.raw = Some(PathBuf::from(value()?)),
            "--repeat" => a.repeat = num(value()?, &flag)?.max(1) as u32,
            "--clock-step" => a.clock_step = num(value()?, &flag)?,
            "--dump-apps" => a.dump_apps = Some(PathBuf::from(value()?)),
            "--list" => a.list = true,
            "--cleanup" => a.cleanup = true,
            "--speakers" => a.speakers = true,
            "--cleanup-rounds" => a.cleanup_rounds = num(value()?, &flag)?.max(1) as u32,
            "--locale" => a.locale = Some(value()?),
            "--cleanup-slot" => a.cleanup_slot = Some(num(value()?, &flag)?),
            "--chat-slot" => a.chat_slot = Some(num(value()?, &flag)?),
            "--cleanup-layout" => {
                let l = value()?;
                a.cleanup_layout = Some(
                    super::cleanup::LAYOUTS
                        .iter()
                        .copied()
                        .find(|x| *x == l)
                        .ok_or_else(|| format!("--cleanup-layout {}, not {l}", super::cleanup::LAYOUTS.join("|")))?,
                );
            }
            "--cleanup-quick" => a.cleanup_quick = true,
            "--cleanup-limit" => a.cleanup_limit = Some(num(value()?, &flag)?.max(1) as usize),
            "--brief" => a.brief = true,
            "--bench" => {
                let b = value()?;
                if !super::serving::BENCHES.contains(&b.as_str()) {
                    return Err(format!("--bench {}, not {b}", super::serving::BENCHES.join("|")));
                }
                a.bench = Some(b);
            }
            "--bench-rounds" => a.bench_rounds = num(value()?, &flag)?.max(1) as u32,
            "--bench-history" => a.bench_history = num(value()?, &flag)? as usize,
            "--temperature" => {
                let v = value()?;
                a.temperature = Some(v.trim().parse::<f64>().map_err(|_| format!("--temperature: not a number: {v}"))?);
            }
            "--set" => {
                let v = value()?;
                let (k, j) = v.split_once('=').ok_or_else(|| format!("--set key=json, not {v}"))?;
                let j: Value = serde_json::from_str(j.trim()).unwrap_or_else(|_| json!(j.trim()));
                a.set.push((k.trim().to_string(), j));
            }
            "--help" | "-h" => return Err(String::new()),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(a)
}

/// The harness's `main` (after `yap.exe eval`).
pub fn main(argv: Vec<String>) -> i32 {
    let args = match parse(argv) {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("{e}\n");
            }
            eprintln!("{USAGE}");
            return if e.is_empty() { 0 } else { 2 };
        }
    };
    if let Some(p) = &args.dump_apps {
        return dump_apps(p);
    }
    // Raw requests alone need no scenarios (and --only then names requests).
    let scenarios = if (args.models.is_empty() || args.cleanup || args.speakers || args.brief || args.bench.is_some()) && !args.list {
        Vec::new()
    } else {
        match load_scenarios(&args) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{e}");
                return 2;
            }
        }
    };
    if args.list {
        for s in &scenarios {
            println!(
                "{:<34} {:<22} {}{}",
                s.name,
                s.tags.join(","),
                if s.needs_web { "[needs web] " } else { "" },
                s.description
            );
        }
        return 0;
    }
    if args.models.is_empty() && args.raw.is_none() && !args.speakers {
        eprintln!("Give at least one --model (or --raw).\n\n{USAGE}");
        return 2;
    }

    // The run folder, and the run's own data dir — before anything reads one.
    let out = args
        .out
        .clone()
        .or_else(|| std::env::var_os("YAP_EVAL_OUT").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("test-results").join("evals"));
    let label = args.label.clone().unwrap_or_else(|| {
        if args.models.is_empty() && args.speakers {
            "speakers".to_string()
        } else if args.models.is_empty() {
            "raw".to_string()
        } else {
            args.models.iter().map(|m| model_short(m)).collect::<Vec<_>>().join("+")
        }
    });
    let run_dir = out.join(format!("{}-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"), slug(&label)));
    if let Err(e) = std::fs::create_dir_all(run_dir.join("data")).and_then(|_| std::fs::create_dir_all(run_dir.join("apps"))) {
        eprintln!("Can't make {}: {e}", run_dir.display());
        return 2;
    }
    // Absolute, without Windows' "\\?\" prefix (paths in the logs read as typed).
    let run_dir = std::fs::canonicalize(&run_dir)
        .ok()
        .and_then(|p| p.to_str().map(|s| PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(s))))
        .unwrap_or(run_dir);
    if !crate::portable::use_data_dir(run_dir.join("data")) {
        eprintln!("The data dir was already decided; refusing to run against it.");
        return 2;
    }
    // Test mode (as the e2e suite): apps are stand-ins and an "open" is only
    // recorded; nothing launches, pastes or notifies.
    std::env::set_var("YAP_E2E", "1");
    // Test mode never reads Windows' region (about_you.rs): `--locale`
    // stands in for it ("en-GB", or the JSON `YAP_E2E_LOCALE` takes).
    if let Some(l) = &args.locale {
        std::env::set_var("YAP_E2E_LOCALE", l);
    }
    if !crate::e2e::active() {
        eprintln!("Test mode didn't switch on; refusing to run (an app could really open).");
        return 2;
    }
    super::ACTIVE.store(true, std::sync::atomic::Ordering::SeqCst);
    init_tracing(&run_dir.join("yap.log"));

    let keys = load_keys(args.keys_from_yap);
    if let Err(e) = record::open(Some(&run_dir.join("transcript.jsonl")), keys.values().cloned().collect(), None) {
        eprintln!("{e}");
        return 2;
    }
    record::set_timeout(args.timeout.map(Duration::from_secs));
    record::set_run_body(run_body(&args));
    println!("Run folder: {}", run_dir.display());
    let code = tauri::async_runtime::block_on(run_all(&args, &run_dir, &keys, scenarios));
    record::close();
    println!("\nRun folder: {}", run_dir.display());
    code
}

/// The fields `--temperature` and `--set` lay over every request body.
fn run_body(args: &Args) -> serde_json::Map<String, Value> {
    let mut m = serde_json::Map::new();
    if let Some(t) = args.temperature {
        m.insert("temperature".into(), json!(t));
    }
    for (k, v) in &args.set {
        m.insert(k.clone(), v.clone());
    }
    m
}

/// `--dump-apps <path>` (the harness runs itself with it, outside test
/// mode): this PC's Start menu apps, read-only, as JSON.
fn dump_apps(path: &Path) -> i32 {
    let apps = crate::apps::apps(true);
    let list: Vec<AppEntry> = apps.iter().map(|a| AppEntry { id: a.id.clone(), name: a.name.clone() }).collect();
    match serde_json::to_string_pretty(&list).map_err(|e| e.to_string()).and_then(|s| std::fs::write(path, s).map_err(|e| e.to_string())) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn init_tracing(path: &Path) {
    use tracing_subscriber::EnvFilter;
    let Ok(file) = std::fs::File::create(path) else { return };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,yap_lib=debug"));
    let _ = tracing_subscriber::fmt()
        .with_writer(std::sync::Mutex::new(file))
        .with_ansi(false)
        .with_env_filter(filter)
        .try_init();
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '.' {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').chars().take(80).collect()
}

/// "groq:openai/gpt-oss-20b" → "gpt-oss-20b"; "local:C:\…\x.gguf" → "x".
fn model_short(spec: &str) -> String {
    let rest = spec.split_once(':').map_or(spec, |(_, r)| r);
    let rest = rest.split('#').next().unwrap_or(rest);
    let last = rest.rsplit(['/', '\\', ',']).next().unwrap_or(rest);
    last.trim_end_matches(".gguf").to_string()
}

/// The keys: environment variables first, then (with `--keys-from-yap`)
/// what Yap saved, read straight from its config.json — never through
/// `config::load`, which may save, and never written anywhere.
fn load_keys(from_yap: bool) -> HashMap<String, String> {
    let mut keys = HashMap::new();
    if from_yap {
        if let Some(path) = dirs::data_dir().map(|d| d.join("yap").join("config.json")) {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(cfg) = serde_json::from_str::<Value>(&text) {
                    let mut put = |provider: &str, key: &str| {
                        if !provider.is_empty() && !key.trim().is_empty() {
                            keys.entry(provider.to_string()).or_insert_with(|| key.trim().to_string());
                        }
                    };
                    if let Some(map) = cfg["ppApiKeys"].as_object() {
                        for (p, k) in map {
                            put(p, k.as_str().unwrap_or(""));
                        }
                    }
                    put(cfg["ppProvider"].as_str().unwrap_or(""), cfg["ppApiKey"].as_str().unwrap_or(""));
                    // Web search's keys ("exa", "tavily").
                    if let Some(map) = cfg["webSearchKeys"].as_object() {
                        for (p, k) in map {
                            put(p, k.as_str().unwrap_or(""));
                        }
                    }
                    if let Some(scopes) = cfg["llmScopes"].as_object() {
                        for s in scopes.values() {
                            put(s["provider"].as_str().unwrap_or(""), s["apiKey"].as_str().unwrap_or(""));
                            if let Some(map) = s["apiKeys"].as_object() {
                                for (p, k) in map {
                                    put(p, k.as_str().unwrap_or(""));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    for (provider, var) in [
        ("groq", "GROQ_API_KEY"),
        ("openai", "OPENAI_API_KEY"),
        ("anthropic", "ANTHROPIC_API_KEY"),
        ("openrouter", "OPENROUTER_API_KEY"),
        ("gemini", "GEMINI_API_KEY"),
        ("custom", "YAP_EVAL_CUSTOM_KEY"),
        ("exa", "EXA_API_KEY"),
        ("tavily", "TAVILY_API_KEY"),
    ] {
        if let Ok(k) = std::env::var(var) {
            if !k.trim().is_empty() {
                keys.insert(provider.to_string(), k.trim().to_string());
            }
        }
    }
    keys
}

fn scenarios_dir(args: &Args) -> PathBuf {
    if let Some(p) = args.scenarios.clone().or_else(|| std::env::var_os("YAP_EVAL_SCENARIOS").map(PathBuf::from)) {
        return p;
    }
    for c in ["evals/scenarios", "../evals/scenarios"] {
        if Path::new(c).is_dir() {
            return PathBuf::from(c);
        }
    }
    PathBuf::from("evals/scenarios")
}

fn load_scenarios(args: &Args) -> Result<Vec<Scenario>, String> {
    let dir = scenarios_dir(args);
    let files: Vec<PathBuf> = if dir.is_file() {
        vec![dir.clone()]
    } else {
        let mut f: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| format!("can't read scenarios in {}: {e} (run from the repo, or pass --scenarios)", dir.display()))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        f.sort();
        f
    };
    let mut out = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        let mut s: Scenario = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", f.display()))?;
        s.file = f.to_string_lossy().into_owned();
        out.push(s);
    }
    let mut names = std::collections::HashSet::new();
    for s in &out {
        if !names.insert(s.name.clone()) {
            return Err(format!("two scenarios are called {}", s.name));
        }
    }
    let only: Vec<String> = args.only.iter().map(|s| s.to_lowercase()).collect();
    out.retain(|s| only.is_empty() || only.iter().any(|o| s.name.to_lowercase().contains(o)));
    out.retain(|s| args.tags.is_empty() || s.tags.iter().any(|t| args.tags.contains(t)));
    if out.is_empty() {
        return Err(format!("no scenarios match in {}", dir.display()));
    }
    Ok(out)
}

/// A model to run, resolved.
struct Model {
    spec: String,
    label: String,
    provider: String,
    base_url: String,
    model: String,
    key: String,
    server: Option<local::Server>,
    gguf: Option<PathBuf>,
    /// What `tools::supports_tools` says (before any --tools).
    tools_rule: bool,
}

async fn prepare(spec: &str, args: &Args, keys: &HashMap<String, String>, run_dir: &Path, models_dir: &Path) -> Result<Model, String> {
    let (kind, rest) = spec.split_once(':').ok_or_else(|| format!("{spec}: write provider:model"))?;
    let kind = kind.to_lowercase();
    let mut m = Model {
        spec: spec.to_string(),
        label: spec.to_string(),
        provider: kind.clone(),
        base_url: String::new(),
        model: rest.to_string(),
        key: keys.get(&kind).cloned().unwrap_or_default(),
        server: None,
        gguf: None,
        tools_rule: false,
    };
    match kind.as_str() {
        "groq" | "openai" | "anthropic" | "openrouter" | "gemini" => {
            m.base_url = crate::ai_model::standard_base_url(&kind).unwrap_or_default().trim_end_matches('/').to_string();
            if m.key.is_empty() {
                return Err(format!("no {kind} key: set {}_API_KEY or pass --keys-from-yap", kind.to_uppercase()));
            }
        }
        "custom" => {
            let (url, model) = rest.rsplit_once(',').ok_or("custom:<base_url>,<model>")?;
            m.base_url = url.trim().trim_end_matches('/').to_string();
            m.model = model.trim().to_string();
        }
        "local" => {
            let runtime = local::runtime(args.llamafile.as_deref(), models_dir).await?;
            let gguf = local::model(rest, models_dir).await?;
            let plan = local::plan(&runtime, &gguf, args.ngl, args.ctx, args.mtp).await;
            println!(
                "Starting llamafile {} ({}) with {} (ngl {}, ctx {}, MTP {}) …",
                plan.runtime,
                runtime.display(),
                gguf.display(),
                args.ngl,
                args.ctx,
                if plan.server().mtp { "on" } else { "off" }
            );
            let server = local::start(&runtime, &gguf, args.ngl, args.ctx, plan, &args.llamafile_args, run_dir, !args.no_warm_up).await?;
            println!("  ready in {:.1} s", server.load_ms as f64 / 1000.0);
            if let Some(w) = &server.warm_up {
                println!("  warm-up: {} ms{}{}", w.ms, if w.cancelled { " (cancelled)" } else { "" }, w.error.as_deref().map(|e| format!(" — {e}")).unwrap_or_default());
            }
            m.provider = "local".into();
            m.base_url = server.base_url.clone();
            // The id Yap would send for this file; its size ("-1.5b") is
            // what `supports_tools` reads.
            m.model = gguf.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            m.label = format!("local:{} ({})", m.model, if args.ngl == 0 { "CPU".to_string() } else { format!("ngl {}", args.ngl) });
            m.server = Some(server);
            m.gguf = Some(gguf);
        }
        other => return Err(format!("unknown provider \"{other}\" (groq, openai, anthropic, openrouter, gemini, custom, local)")),
    }
    record::set_tools_forced(None);
    m.tools_rule = crate::tools::supports_tools(&m.provider, &m.model);
    if !m.key.is_empty() {
        record::add_secret(&m.key);
    }
    Ok(m)
}

/// Does scenario `s` turn "Look things up on the web" on?
fn web_on(s: &Scenario) -> bool {
    s.needs_web || s.web.is_some()
}

/// A scenario's web search: on or off, the service, its key ("" = the
/// keyless lookup), and whether a stand-in answers.
struct WebPlan {
    on: bool,
    service: String,
    key: String,
    standin: bool,
    /// "The AI's own search" for a cloud model (web/native.rs).
    native: bool,
}

impl WebPlan {
    fn of(s: &Scenario, keys: &HashMap<String, String>) -> WebPlan {
        let spec = s.web.clone().unwrap_or_default();
        let standin = spec.mode.as_deref() == Some("standin");
        // The app's default (on), except where the scenario tests Yap's own
        // search: a stand-in web, or `key: false`.
        let native = spec.native.unwrap_or(!standin && s.web.as_ref().is_none_or(|w| w.key));
        let service = spec.service.clone().unwrap_or_else(|| {
            if !keys.contains_key("exa") && keys.contains_key("tavily") { "tavily" } else { "exa" }.to_string()
        });
        let key = if !spec.key {
            String::new()
        } else if standin {
            "standin-key".to_string()
        } else {
            keys.get(&service).cloned().unwrap_or_default()
        };
        WebPlan { on: web_on(s), service, key, standin, native }
    }

    /// For the logs: where searches go (never the key).
    fn describe(&self) -> Value {
        if !self.on {
            return Value::Null;
        }
        json!({
            "mode": if self.standin { "standin" } else { "live" },
            "service": self.service,
            "backend": if self.key.is_empty() { "keyless (Wikipedia + game wikis)".to_string() } else { self.service.clone() },
            "native": self.native,
        })
    }
}

/// Chat's settings for `m` and scenario `s`: the Chat scope on the model,
/// Settings' default persona, the scenario's app and web settings.
fn settings_for(m: &Model, s: &Scenario, apps: &[AppEntry], web: &WebPlan) -> crate::config::YapConfig {
    let mut cfg = crate::config::YapConfig {
        pp_provider: m.provider.clone(),
        pp_base_url: m.base_url.clone(),
        pp_api_key: m.key.clone(),
        pp_model: m.model.clone(),
        open_apps: s.settings.open_apps,
        memory_sensitive: s.settings.memory_sensitive,
        memory_search_chats: s.settings.memory_search_chats,
        about_name: s.settings.about_name.clone(),
        about_me: s.settings.about_me.clone(),
        region_country: s.settings.region.clone(),
        web_search: web.on,
        web_search_native: web.native,
        web_search_service: web.service.clone(),
        ..Default::default()
    };
    if web.on && !web.key.is_empty() {
        cfg.web_search_keys.insert(web.service.clone(), web.key.clone());
    }
    cfg.llm_scopes.insert(
        "chat".to_string(),
        crate::config::LlmScope {
            enabled: true,
            provider: m.provider.clone(),
            base_url: m.base_url.clone(),
            model: m.model.clone(),
            api_key: m.key.clone(),
            prompt: s.settings.persona.clone().unwrap_or_else(|| DEFAULT_PERSONA.to_string()),
            ..Default::default()
        },
    );
    let id_of = |name: &str| apps.iter().find(|a| a.name.eq_ignore_ascii_case(name)).map(|a| a.id.clone());
    cfg.app_nicknames = s
        .settings
        .app_nicknames
        .iter()
        .filter_map(|n| {
            id_of(&n.app).map(|id| crate::config::AppNickname { say: n.say.clone(), app: id, name: n.app.clone() })
        })
        .collect();
    cfg.apps_blocked = s.settings.apps_blocked.iter().filter_map(|n| id_of(n)).collect();
    cfg
}

/// The app list a scenario names, with ids filled in.
fn resolve_apps(apps: &Apps, scenarios: &Path, run_dir: &Path, this_pc: &mut Option<Vec<AppEntry>>) -> Result<Vec<AppEntry>, String> {
    let mut list = match apps {
        Apps::List(l) => l.clone(),
        Apps::Named(n) if n == "this_pc" => {
            if this_pc.is_none() {
                let path = run_dir.join("apps").join("this-pc.json");
                let exe = std::env::current_exe().map_err(|e| e.to_string())?;
                let status = std::process::Command::new(exe)
                    .args(["eval", "--dump-apps"])
                    .arg(&path)
                    .env_remove("YAP_E2E")
                    .status()
                    .map_err(|e| e.to_string())?;
                if !status.success() {
                    return Err("couldn't read this PC's apps".to_string());
                }
                let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                *this_pc = Some(serde_json::from_str(&text).map_err(|e| e.to_string())?);
            }
            this_pc.clone().unwrap_or_default()
        }
        Apps::Named(n) => {
            let dir = if scenarios.is_file() { scenarios.parent().unwrap_or(Path::new(".")) } else { scenarios };
            let path = dir.join("..").join("apps").join(format!("{n}.json"));
            let text = std::fs::read_to_string(&path).map_err(|e| format!("apps \"{n}\": {}: {e}", path.display()))?;
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?
        }
    };
    for a in &mut list {
        if a.id.trim().is_empty() {
            a.id = format!("{}.lnk", a.name);
        }
    }
    Ok(list)
}

/// Put the scenario's notes and folders in the (fresh, in-memory) store,
/// its memory and its earlier chats.
fn seed(s: &Scenario) -> Result<(), String> {
    crate::notes::eval_reset();
    crate::memory::eval_reset();
    crate::chats::eval_reset();
    for m in &s.seed.memory {
        use crate::memory::ops::{Action, Change, Origin, Rules, Section};
        let section = Section::parse(&m.section).ok_or_else(|| format!("memory section \"{}\"", m.section))?;
        let change = Change {
            action: Action::Add,
            id: None,
            section: Some(section),
            name: m.title.clone(),
            about: (!m.summary.is_empty()).then(|| m.summary.clone()),
            facts: m.details.clone(),
            remove_facts: vec![],
            say: String::new(),
        };
        let rules = Rules { sensitive_ok: true, origin: Origin::new("chat", None, "Seed"), now: crate::memory::now_secs(), max_changes: 1, grounding: None };
        if crate::memory::change(&[change], &rules).done.is_empty() {
            return Err(format!("memory seed \"{}\" wasn't kept", m.title));
        }
    }
    for c in &s.seed.chats {
        let conv = crate::chats::create(&c.title);
        for m in &c.messages {
            crate::chats::append(conv.id, &m.role, &m.text, &[], None, false)?;
        }
    }
    for f in &s.seed.folders {
        crate::notes::folder_create(f);
    }
    for n in &s.seed.notes {
        let note = crate::notes::create(&n.title, &n.content, "manual", &n.folder);
        if n.meeting || !n.transcript.is_empty() {
            crate::notes::mark_meeting(note.id)?;
        }
        if !n.participants.is_empty() {
            crate::notes::update(note.id, None, None, None, Some(n.participants.clone()))?;
        }
        if !n.enhanced.is_empty() {
            crate::notes::set_enhanced(note.id, &n.enhanced, &crate::notes::content_hash(&n.content))?;
        }
        if !n.transcript.is_empty() {
            let lines: Vec<crate::notes::TranscriptSegment> = n
                .transcript
                .iter()
                .map(|l| crate::notes::TranscriptSegment {
                    source: l.source.clone(),
                    text: l.text.clone(),
                    ts: l.ts,
                    ..Default::default()
                })
                .collect();
            crate::notes::append_transcript(note.id, &lines)?;
        }
    }
    Ok(())
}

/// One scenario on one model: its result row.
#[allow(clippy::too_many_arguments)]
async fn run_scenario(
    m: &Model,
    s: &Scenario,
    args: &Args,
    keys: &HashMap<String, String>,
    run_dir: &Path,
    this_pc: &mut Option<Vec<AppEntry>>,
    rep: u32,
) -> Value {
    let started = Instant::now();
    let name = if args.repeat > 1 { format!("{}#{}", s.name, rep + 1) } else { s.name.clone() };
    record::set_context(json!({ "model": m.label, "scenario": name }));
    let apps = match resolve_apps(&s.apps, &scenarios_dir(args), run_dir, this_pc) {
        Ok(a) => a,
        Err(e) => return error_row(m, &name, &format!("apps: {e}")),
    };
    let apps_file = run_dir.join("apps").join(format!("{}.json", slug(&s.name)));
    if let Err(e) = std::fs::write(&apps_file, serde_json::to_string(&apps).unwrap_or_default()) {
        return error_row(m, &name, &format!("apps file: {e}"));
    }
    std::env::set_var("YAP_E2E_APPS", &apps_file);
    // The web: fresh caches; a stand-in server, or the real services.
    let web = WebPlan::of(s, keys);
    crate::web::clear_caches();
    let _standin = if web.on && web.standin {
        match Standin::start(&s.web.clone().unwrap_or_default()) {
            Ok(st) => {
                for (k, v) in st.env() {
                    std::env::set_var(k, v);
                }
                Some(st)
            }
            Err(e) => return error_row(m, &name, &e),
        }
    } else {
        for k in ["YAP_EXA_URL", "YAP_TAVILY_URL", "YAP_WEB_STANDIN"] {
            std::env::remove_var(k);
        }
        None
    };
    record::set_web_live(web.on && !web.standin);
    record::set_config(Some(settings_for(m, s, &apps, &web)));
    let forced = match s.tools.as_deref() {
        Some("on") => Some(true),
        Some("off") => Some(false),
        _ => args.tools,
    };
    record::set_tools_forced(forced);
    let tools = forced.unwrap_or(m.tools_rule);
    if let Err(e) = seed(s) {
        return error_row(m, &name, &format!("seed: {e}"));
    }
    let _ = crate::apps::e2e_opened_apps(Some(true));
    record::write(
        "scenario_start",
        json!({
            "description": s.description,
            "file": s.file,
            "tools": tools,
            "tools_forced": forced,
            "apps": apps.len(),
            "notes": crate::notes::all().len(),
            "folders": crate::notes::folders(),
            "web": web.describe(),
        }),
    );

    let mut conversation: Option<u64> = None;
    let mut seen: Vec<Observed> = Vec::new();
    let mut fails: Vec<String> = Vec::new();
    let mut warns: Vec<String> = Vec::new();
    let mut turns: Vec<Value> = Vec::new();
    let mut earlier_numbers: Vec<usize> = Vec::new();
    let mut previous_numbers: Vec<usize> = Vec::new();
    for (i, turn) in s.turns.iter().enumerate() {
        record::set_context(json!({ "model": m.label, "scenario": name, "turn": i + 1 }));
        let voice = turn.voice.unwrap_or(s.voice);
        record::write("turn_start", json!({ "user": turn.user, "voice": voice }));
        if i > 0 && args.clock_step > 0 {
            crate::eval::clock_forward(args.clock_step as i64 * 60);
        }
        record::begin_turn();
        let t0 = Instant::now();
        let memory_before = crate::memory::items();
        // Asked out loud = the Voice Assistant's turn (its shorter history,
        // its spoken-answer prompt).
        let scope = voice.then_some("assistant");
        // Streamed, as the Chat view gets it (`YAP_EVAL_STREAM=off`: whole
        // answers, as before 22 Oct 2026): when the first words showed.
        let watch = FirstWords::new();
        let streamed = std::env::var("YAP_EVAL_STREAM").as_deref() != Ok("off");
        let result = if turn.user.trim().is_empty() {
            // A memory action alone (learn, tell, import): no chat turn.
            Ok(None)
        } else {
            let watcher = streamed.then_some(&watch as &dyn crate::stream::Sink);
            crate::commands::chat_turn(None, conversation, &turn.user, scope, voice, |_, _| {}, watcher, None).await.map(Some)
        };
        let first_words = watch.at();
        // Memory's passes after the turn: what a quiet chat, "Tell Yap what
        // to change" or an import would do.
        let mut memory_error: Option<String> = None;
        let mut memory_pass = Value::Null;
        if turn.learn {
            let id = result.as_ref().ok().and_then(|t| t.as_ref().map(|t| t.conversation_id)).or(conversation);
            match id {
                Some(id) => {
                    // What the pass alone cost: its calls and time, apart
                    // from the chat turn's.
                    let (s0, p0) = (record::turn_so_far(), Instant::now());
                    if let Err(e) = crate::memory::learn::learn_from_chat(id, false).await {
                        memory_error = Some(format!("learning: {e}"));
                    }
                    let s1 = record::turn_so_far();
                    memory_pass = json!({
                        "ms": p0.elapsed().as_millis() as u64,
                        "calls": s1.requests - s0.requests,
                        "model_ms": s1.model_ms - s0.model_ms,
                        "prompt_tokens": s1.prompt_tokens - s0.prompt_tokens,
                        "prompt_eval_tokens": s1.prompt_eval_tokens - s0.prompt_eval_tokens,
                        "prompt_eval_ms": s1.prompt_eval_ms - s0.prompt_eval_ms,
                        "gen_tokens": s1.gen_tokens - s0.gen_tokens,
                        "gen_ms": s1.gen_ms - s0.gen_ms,
                    });
                    record::write("memory_pass", memory_pass.clone());
                }
                None => memory_error = Some("learning: no chat to read".to_string()),
            }
        }
        for (kind, text) in [(crate::memory::learn::Kind::Tell, &turn.tell), (crate::memory::learn::Kind::Import, &turn.import)] {
            if let Some(text) = text {
                match crate::memory::learn::propose(text, kind).await {
                    Ok(p) => {
                        record::write("memory_proposal", json!({ "changes": p.changes }));
                        if let Err(e) = crate::memory::learn::apply_proposal(p.id, None) {
                            memory_error = Some(e);
                        }
                    }
                    Err(e) => memory_error = Some(format!("{kind:?}: {e}")),
                }
            }
        }
        let memory_after = crate::memory::items();
        let memory_done = scenario::memory_diff(&memory_before, &memory_after);
        let memory_gained = scenario::memory_gained(&memory_before, &memory_after);
        if !memory_done.is_empty() {
            record::write("memory_changed", json!({ "changes": memory_done, "gained": memory_gained }));
        }
        let stats = record::end_turn();
        let opened = crate::apps::e2e_opened_apps(Some(true))
            .unwrap_or_default()
            .iter()
            .filter_map(|v| v["name"].as_str().map(str::to_string))
            .collect();
        let mut o = Observed { stats, opened, memory_done, memory_gained, ..Default::default() };
        // What a follow-up's "[1]" points at — for "where did you get that?",
        // only the answer it asks about — and whether the answer before it
        // searched (a "did you look that up?" asks about that one).
        o.earlier_sources =
            if crate::web::about_the_answer(&turn.user) { previous_numbers.clone() } else { earlier_numbers.clone() };
        o.earlier_searched =
            seen.last().is_some_and(|p| p.stats.tool_calls.iter().any(|c| crate::web::TOOL_NAMES.contains(&c.name.as_str())));
        match result {
            Ok(Some(t)) => {
                conversation = Some(t.conversation_id);
                o.reply = t.reply;
                o.chips = t.tools_used;
                o.fast_path = t.model.is_none();
                o.sources = t.sources.iter().map(|x| (x.title.clone(), x.url.clone())).collect();
                previous_numbers = t.sources.iter().map(|x| x.n).collect();
                if !t.sources.is_empty() {
                    earlier_numbers = previous_numbers.clone();
                }
            }
            Ok(None) => {}
            Err(e) => {
                o.error = Some(e);
                previous_numbers.clear();
            }
        }
        if let Some(e) = memory_error {
            o.error = Some(o.error.take().map(|x| format!("{x}; {e}")).unwrap_or(e));
        }
        let ms = t0.elapsed().as_millis() as u64;
        let f = scenario::check(&turn.expect, &o);
        // An error is the turn's failure already; soft checks don't repeat it.
        let w = scenario::check(&scenario::Expect { allow_error: true, ..turn.soft.clone() }, &o);
        let mut j = scenario::observed_json(&o);
        j["user"] = json!(turn.user);
        j["ms"] = json!(ms);
        let touches_memory = turn.learn || turn.tell.is_some() || turn.import.is_some() || !o.memory_gained.is_empty();
        let score = scenario::memory_score(&turn.expect, &turn.soft, &o);
        if touches_memory || score.recall_total > 0 {
            j["memory_score"] = json!({
                "recall_hits": score.recall_hits,
                "recall_total": score.recall_total,
                "false": score.false_texts,
            });
        }
        if !memory_pass.is_null() {
            j["memory_pass"] = memory_pass.clone();
        }
        // What a streamed answer would have shown first: everything before
        // the answering call, then its prompt and first token (local models).
        if o.stats.final_first_token_ms > 0.0 {
            j["ttft_est_ms"] = json!(ttft_estimate(ms, o.stats.final_call_ms, o.stats.final_first_token_ms));
        }
        // When the first words of the answer showed, streamed (measured,
        // less the run's own pacing before them, as "Mean turn" leaves it
        // out).
        if let Some(t) = first_words.filter(|_| o.error.is_none()) {
            j["first_words_ms"] = json!(t);
        }
        j["paced_ms"] = json!(o.stats.pacing_wait_ms);
        j["fails"] = json!(f);
        j["warnings"] = json!(w);
        record::write("turn_end", j.clone());
        let short = |r: &String| if s.turns.len() > 1 { format!("turn {}: {r}", i + 1) } else { r.clone() };
        fails.extend(f.iter().map(short));
        warns.extend(w.iter().map(short));
        turns.push(j);
        seen.push(o);
    }
    record::set_context(json!({ "model": m.label, "scenario": name }));
    let all = Observed::merge(&seen);
    // A turn's error was reported with that turn.
    let end_f = scenario::check(&scenario::Expect { allow_error: true, ..s.expect_end.clone() }, &all);
    let end_w = scenario::check(&scenario::Expect { allow_error: true, ..s.soft_end.clone() }, &all);
    fails.extend(end_f.iter().map(|r| format!("end: {r}")));
    warns.extend(end_w.iter().map(|r| format!("end: {r}")));
    let st = &all.stats;
    // Memory's precision and recall over the turns, and its passes' cost.
    let scores: Vec<&Value> = turns.iter().filter_map(|t| t.get("memory_score")).collect();
    let passes: Vec<&Value> = turns.iter().filter_map(|t| t.get("memory_pass")).collect();
    let false_texts: Vec<Value> = scores.iter().flat_map(|s| s["false"].as_array().cloned().unwrap_or_default()).collect();
    let mut row = json!({
        "model": m.label,
        "scenario": name,
        "tags": s.tags,
        "status": if fails.is_empty() { "pass" } else { "fail" },
        "reasons": fails,
        "warnings": warns,
        "tools": tools,
        "web": web.describe(),
        "sources": all.sources.iter().map(|(t, u)| json!({ "title": t, "url": u })).collect::<Vec<_>>(),
        "turns": s.turns.len(),
        "steps": st.requests,
        "retries": st.retries,
        "tool_calls": st.tool_calls.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
        "fast_path_turns": seen.iter().filter(|o| o.fast_path).count(),
        "opened": all.opened,
        "prompt_tokens": st.prompt_tokens,
        "completion_tokens": st.completion_tokens,
        "total_tokens": st.total_tokens,
        "cached_tokens": st.cached_tokens,
        "reasoning_tokens": st.reasoning_tokens,
        "model_ms": st.model_ms,
        "slowest_call_ms": st.slowest_call_ms,
        "calls_over_20s": st.calls_over_20s,
        "wall_ms": started.elapsed().as_millis() as u64,
        "rate_limited": st.rate_limited,
        "rate_limit_wait_ms": st.rate_limit_wait_ms,
        "pacing_wait_ms": st.pacing_wait_ms,
        "prompt_eval_tokens": st.prompt_eval_tokens,
        "prompt_eval_ms": st.prompt_eval_ms,
        "gen_tokens": st.gen_tokens,
        "gen_ms": st.gen_ms,
        "draft_tokens": st.draft_tokens,
        "draft_accepted": st.draft_accepted,
        "ttft_est_ms": turns.iter().filter_map(|t| t["ttft_est_ms"].as_u64()).sum::<u64>(),
        "ttft_turns": turns.iter().filter(|t| t["ttft_est_ms"].is_u64()).count(),
        // Streamed: each answered turn's first words (measured), and every
        // turn's whole time (what a whole answer waited for its first words).
        "first_words_ms": turns.iter().filter_map(|t| t["first_words_ms"].as_u64()).collect::<Vec<_>>(),
        "turn_ms": turns
            .iter()
            .filter(|t| t["error"].is_null())
            .filter_map(|t| Some(t["ms"].as_u64()?.saturating_sub(t["paced_ms"].as_u64().unwrap_or(0))))
            .collect::<Vec<_>>(),
        "words": all.words(),
        "replies": seen.iter().map(|o| o.reply.clone()).collect::<Vec<_>>(),
        "errors": seen.iter().filter_map(|o| o.error.clone()).collect::<Vec<_>>(),
        "turn_details": turns,
    });
    if !scores.is_empty() || !passes.is_empty() {
        let n = |v: &Value, k: &str| v[k].as_u64().unwrap_or(0);
        row["memory_false"] = json!(false_texts.len());
        row["memory_false_texts"] = json!(false_texts);
        row["memory_recall_hits"] = json!(scores.iter().map(|s| n(s, "recall_hits")).sum::<u64>());
        row["memory_recall_total"] = json!(scores.iter().map(|s| n(s, "recall_total")).sum::<u64>());
        row["memory_passes"] = json!(passes.len());
        row["memory_pass_calls"] = json!(passes.iter().map(|p| n(p, "calls")).sum::<u64>());
        row["memory_pass_ms"] = json!(passes.iter().map(|p| n(p, "ms")).sum::<u64>());
        row["memory_pass_max_ms"] = json!(passes.iter().map(|p| n(p, "ms")).max().unwrap_or(0));
    }
    record::write("scenario_end", row.clone());
    row
}

/// When a streamed turn's first words showed (a `stream::Sink`): ms after
/// the turn began, less the run's pacing until then (Groq's free plan's
/// waits are the harness's, not Yap's).
struct FirstWords {
    start: Instant,
    at: std::sync::Mutex<Option<u64>>,
}

impl FirstWords {
    fn new() -> FirstWords {
        FirstWords { start: Instant::now(), at: std::sync::Mutex::new(None) }
    }

    fn at(&self) -> Option<u64> {
        *self.at.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl crate::stream::Sink for FirstWords {
    fn send(&self, ev: crate::stream::Event) {
        if let crate::stream::Event::Text { text } = &ev {
            let mut at = self.at.lock().unwrap_or_else(|p| p.into_inner());
            if at.is_none() && !text.trim().is_empty() {
                let ms = self.start.elapsed().as_millis() as u64;
                *at = Some(ms.saturating_sub(record::turn_so_far().pacing_wait_ms));
            }
        }
    }
}

/// A turn's time to its answer's first words, had the answer been streamed:
/// the turn's wall time less the answering call's, plus that call's prompt
/// and first token (llama.cpp's timings). Pure.
fn ttft_estimate(turn_ms: u64, final_call_ms: u64, first_token_ms: f64) -> u64 {
    turn_ms.saturating_sub(final_call_ms) + first_token_ms.round() as u64
}

fn error_row(m: &Model, scenario: &str, error: &str) -> Value {
    let row = json!({ "model": m.label, "scenario": scenario, "status": "error", "reasons": [error], "warnings": [] });
    record::write("scenario_end", row.clone());
    row
}

fn skip_row(label: &str, scenario: &Scenario, why: &str) -> Value {
    json!({ "model": label, "scenario": scenario.name, "tags": scenario.tags, "status": "skip", "reasons": [why], "warnings": [] })
}

async fn run_all(args: &Args, run_dir: &Path, keys: &HashMap<String, String>, scenarios: Vec<Scenario>) -> i32 {
    let started = chrono::Local::now();
    let models_dir = args
        .models_dir
        .clone()
        .or_else(|| std::env::var_os("YAP_EVAL_MODELS").map(PathBuf::from))
        .unwrap_or_else(|| run_dir.parent().and_then(Path::parent).unwrap_or(run_dir).join("eval-models"));
    record::write(
        "run",
        json!({
            "started": started.to_rfc3339(),
            "dir": run_dir,
            "models": args.models,
            "scenarios": scenarios.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
            "keys_for": keys.keys().collect::<Vec<_>>(),
            "tools": args.tools,
            "tokens_per_minute": args.tokens_per_minute,
            "timeout_s": args.timeout,
            "ngl": args.ngl,
            "ctx": args.ctx,
            "locale": std::env::var("YAP_E2E_LOCALE").ok(),
            "commit": git_commit(),
        }),
    );

    let mut summary = json!({
        "started": started.to_rfc3339(),
        "dir": run_dir,
        "commit": git_commit(),
        "args": {
            "models": args.models, "tools": args.tools, "tokens_per_minute": args.tokens_per_minute,
            "max_total_tokens": args.max_total_tokens, "timeout_s": args.timeout, "ngl": args.ngl, "ctx": args.ctx,
            "llamafile_args": args.llamafile_args, "mtp": args.mtp, "no_warm_up": args.no_warm_up, "repeat": args.repeat, "no_web": args.no_web,
        },
    });
    let mut failed = false;

    if let Some(raw) = &args.raw {
        println!("Raw requests from {} …", raw.display());
        record::set_tokens_per_minute(args.tokens_per_minute.or(Some(8_000)));
        match super::spike::run(raw, keys, &args.only).await {
            Ok(results) => {
                let md = record::redact(&super::spike::markdown(&results));
                let _ = std::fs::write(run_dir.join("spike.md"), &md);
                println!("{md}");
                summary["spike"] = json!(results);
            }
            Err(e) => {
                eprintln!("Raw requests failed: {e}");
                summary["spike_error"] = json!(e);
                failed = true;
            }
        }
    }

    let bench = if args.cleanup {
        let path = scenarios_dir(args).join("..").join("cleanup").join("dictations.json");
        match super::cleanup::load(&path) {
            Ok(b) => Some(b),
            Err(e) => {
                eprintln!("{e}");
                return 2;
            }
        }
    } else {
        None
    };
    let speaker_meetings = if args.speakers {
        let dir = scenarios_dir(args).join("..").join("speakers");
        match super::speakers::load(&dir) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("{e}");
                return 2;
            }
        }
    } else {
        None
    };
    let mut speaker_runs: Vec<(String, Value)> = Vec::new();
    if let Some(meetings) = &speaker_meetings {
        println!("\n=== Speaker names: Rust alone, {} meetings ===", meetings.len());
        speaker_runs.push(("Rust only".to_string(), super::speakers::run_rust(meetings)));
    }
    let briefs = if args.brief {
        let dir = scenarios_dir(args).join("..").join("briefs");
        match super::brief::load(&dir, &args.only) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{e}");
                return 2;
            }
        }
    } else {
        Vec::new()
    };
    let mut brief_results: Vec<(String, Value)> = Vec::new();
    let mut cleanups: Vec<(String, Value)> = Vec::new();
    let mut benches: Vec<(String, Value)> = Vec::new();
    let mut rows: Vec<Value> = Vec::new();
    let mut models_info: Vec<Value> = Vec::new();
    let mut this_pc: Option<Vec<AppEntry>> = None;
    for spec in &args.models {
        println!("\n=== {spec} ===");
        record::set_context(json!({ "model": spec }));
        let mut m = match prepare(spec, args, keys, run_dir, &models_dir).await {
            Ok(m) => m,
            Err(e) => {
                let e = record::redact(&e);
                eprintln!("  can't run {spec}: {e}");
                record::write("model_error", json!({ "error": e }));
                models_info.push(json!({ "spec": spec, "label": spec, "error": e }));
                failed = true;
                continue;
            }
        };
        let tpm = args.tokens_per_minute.or(if m.provider == "groq" { Some(8_000) } else { None });
        record::set_tokens_per_minute(tpm);
        crate::eval::set_local_on_gpu(m.server.as_ref().map(|_| args.ngl > 0));
        let paced_before = record::paced_ms();
        let model_started = Instant::now();
        let mut used: u64 = 0;
        for s in &scenarios {
            for rep in 0..args.repeat {
                let row = if web_on(s) && args.no_web {
                    skip_row(&m.label, s, "a web scenario (--no-web)")
                } else if args.max_total_tokens.is_some_and(|cap| used >= cap) {
                    skip_row(&m.label, s, "token cap reached (--max-total-tokens)")
                } else {
                    run_scenario(&m, s, args, keys, run_dir, &mut this_pc, rep).await
                };
                used += row["total_tokens"].as_u64().unwrap_or(0);
                print_row(&row);
                if row["status"] == "fail" || row["status"] == "error" {
                    failed = true;
                }
                rows.push(row);
            }
        }
        let mut cleanup = Value::Null;
        if let Some(b) = &bench {
            record::set_context(json!({ "model": m.label, "scenario": "cleanup" }));
            record::set_config(Some(crate::config::YapConfig::default()));
            record::set_tools_forced(None);
            let ep = super::cleanup::Endpoint { base_url: &m.base_url, key: &m.key, model: &m.model, provider: &m.provider };
            let setup = super::cleanup::Setup {
                cleanup: args.cleanup_slot,
                chat: args.chat_slot,
                layout: args.cleanup_layout,
                quick: args.cleanup_quick,
                limit: args.cleanup_limit,
            };
            cleanup = super::cleanup::run(b, &ep, args.cleanup_rounds, setup).await;
            cleanups.push((m.label.clone(), cleanup.clone()));
        }
        let mut speakers_result = Value::Null;
        if let Some(meetings) = &speaker_meetings {
            record::set_config(Some(crate::config::YapConfig::default()));
            record::set_tools_forced(None);
            let ep = super::cleanup::Endpoint { base_url: &m.base_url, key: &m.key, model: &m.model, provider: &m.provider };
            println!("  speaker names, Rust + {} …", m.label);
            speakers_result = super::speakers::run_model(meetings, &ep).await;
            speaker_runs.push((format!("Rust + {}", m.label), speakers_result.clone()));
        }
        let mut brief_result = Value::Null;
        if !briefs.is_empty() {
            record::set_config(Some(crate::config::YapConfig::default()));
            record::set_tools_forced(None);
            let ep = super::brief::Endpoint { base_url: &m.base_url, key: &m.key, model: &m.model, provider: &m.provider };
            brief_result = super::brief::run(&briefs, &ep, args.repeat).await;
            if brief_result["passed"] != brief_result["scenarios"] {
                failed = true;
            }
            brief_results.push((m.label.clone(), brief_result.clone()));
        }
        let mut bench_result = Value::Null;
        if let Some(kind) = &args.bench {
            record::set_context(json!({ "model": m.label, "scenario": format!("bench-{kind}") }));
            record::set_config(Some(crate::config::YapConfig::default()));
            record::set_tools_forced(None);
            let ep = super::cleanup::Endpoint { base_url: &m.base_url, key: &m.key, model: &m.model, provider: &m.provider };
            bench_result = super::serving::run(kind, &ep, args.bench_rounds, args.bench_history).await;
            benches.push((m.label.clone(), bench_result.clone()));
        }
        let mine: Vec<&Value> = rows.iter().filter(|r| r["model"] == json!(m.label)).collect();
        let sum = |k: &str| mine.iter().map(|r| r[k].as_f64().unwrap_or(0.0)).sum::<f64>();
        let mut info = json!({
            "spec": m.spec,
            "label": m.label,
            "provider": m.provider,
            "model": m.model,
            "base_url": m.base_url,
            "tools_rule": m.tools_rule,
            "tokens_per_minute": tpm,
            "paced_ms": record::paced_ms() - paced_before,
            "wall_ms": model_started.elapsed().as_millis() as u64,
            "cleanup": cleanup,
            "speakers": speakers_result,
            "bench": bench_result,
            "brief": brief_result,
        });
        let (drafted, kept) = (sum("draft_tokens"), sum("draft_accepted"));
        if drafted > 0.0 {
            info["draft_acceptance"] = json!((kept / drafted * 1000.0).round() / 1000.0);
        }
        if sum("prompt_eval_ms") > 0.0 {
            info["prompt_tokens_per_s"] = json!((sum("prompt_eval_tokens") / sum("prompt_eval_ms") * 1000.0 * 10.0).round() / 10.0);
        }
        if sum("gen_ms") > 0.0 {
            info["gen_tokens_per_s"] = json!((sum("gen_tokens") / sum("gen_ms") * 1000.0 * 10.0).round() / 10.0);
        }
        if let Some(server) = m.server.as_mut() {
            let gguf = m.gguf.clone().unwrap_or_default();
            info["gguf"] = json!(gguf);
            info["gguf_mb"] = json!(std::fs::metadata(&gguf).map(|x| x.len() / 1_048_576).unwrap_or(0));
            info["load_ms"] = json!(server.load_ms);
            info["peak_ram_mb"] = json!(server.peak_ram_mb());
            info["server_args"] = json!(server.args);
            info["runtime_version"] = json!(server.plan.runtime.to_string());
            info["mtp"] = json!(server.plan.server().mtp);
            info["warm_up"] = json!(server.warm_up);
            info["server_log"] = json!(server.log);
            info["log_facts"] = server.log_facts();
            server.stop();
            println!("  llamafile stopped");
        }
        record::write("model_end", info.clone());
        models_info.push(info);
    }

    summary["finished"] = json!(chrono::Local::now().to_rfc3339());
    summary["models"] = json!(models_info);
    summary["results"] = json!(rows);
    summary["paced_ms"] = json!(record::paced_ms());
    let totals = totals(&rows);
    summary["totals"] = json!(totals);
    if !speaker_runs.is_empty() {
        summary["speakers"] = json!(speaker_runs.iter().map(|(l, v)| json!({ "run": l, "result": v })).collect::<Vec<_>>());
    }
    let text = record::redact(&serde_json::to_string_pretty(&summary).unwrap_or_default());
    let _ = std::fs::write(run_dir.join("summary.json"), text);
    if !cleanups.is_empty() {
        let md = record::redact(&super::cleanup::markdown(&cleanups));
        let _ = std::fs::write(run_dir.join("cleanup.md"), &md);
    }
    if !speaker_runs.is_empty() {
        let md = record::redact(&super::speakers::markdown(&speaker_runs));
        let _ = std::fs::write(run_dir.join("speakers.md"), &md);
        println!("\n{}", md.split("\n## ").next().unwrap_or(""));
    }
    if !brief_results.is_empty() {
        let md = record::redact(&super::brief::markdown(&brief_results));
        let _ = std::fs::write(run_dir.join("brief.md"), &md);
        for (label, r) in &brief_results {
            println!(
                "\n{label}: {}/{} briefs pass; the model wrote {} lines: {} uncited, {} not in their sources, {} with the wrong owner, {} saying an open task was done or a question settled, {} never-say words; {} kept",
                r["passed"], r["scenarios"], r["claims"], r["uncited"], r["ungrounded"], r["swapped"], r["done"], r["planted"], r["kept"]
            );
        }
    }
    if !benches.is_empty() {
        let md = record::redact(&super::serving::markdown(&benches));
        let _ = std::fs::write(run_dir.join("serving.md"), &md);
        println!("\n{md}");
    }
    if !rows.is_empty() {
        let md = record::redact(&markdown(&summary, &rows, &models_info, &totals));
        let _ = std::fs::write(run_dir.join("summary.md"), &md);
        println!("\n{}", table_only(&totals));
        let mem = memory_table(&rows);
        if !mem.is_empty() {
            println!("{}", record::redact(&mem));
        }
    }
    i32::from(failed)
}

fn git_commit() -> Option<String> {
    let out = std::process::Command::new("git").args(["rev-parse", "--short", "HEAD"]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn print_row(r: &Value) {
    let status = r["status"].as_str().unwrap_or("?").to_uppercase();
    let what = match r["status"].as_str() {
        Some("pass") | Some("fail") => format!(
            "{} calls, {} tok, {:.1} s",
            r["steps"],
            r["total_tokens"],
            r["wall_ms"].as_f64().unwrap_or(0.0) / 1000.0
        ),
        _ => String::new(),
    };
    let reason = r["reasons"][0].as_str().map(|s| format!(" | {s}")).unwrap_or_default();
    let reason: String = reason.chars().take(220).collect();
    println!("  {:<4} {:<34} {what}{}", status, r["scenario"].as_str().unwrap_or(""), record::redact(&reason));
}

/// Per model: passed, failed, skipped, calls, tokens, time.
fn totals(rows: &[Value]) -> Vec<Value> {
    let mut order: Vec<String> = Vec::new();
    for r in rows {
        let m = r["model"].as_str().unwrap_or("").to_string();
        if !order.contains(&m) {
            order.push(m);
        }
    }
    order
        .iter()
        .map(|m| {
            let mine: Vec<&Value> = rows.iter().filter(|r| r["model"] == json!(m)).collect();
            let count = |s: &str| mine.iter().filter(|r| r["status"] == json!(s)).count();
            let ran: Vec<&&Value> = mine.iter().filter(|r| r["status"] == json!("pass") || r["status"] == json!("fail")).collect();
            let sum = |k: &str| ran.iter().map(|r| r[k].as_u64().unwrap_or(0)).sum::<u64>();
            let turns = sum("turns").max(1);
            let (pass, fail, err) = (count("pass"), count("fail"), count("error"));
            json!({
                "model": m,
                "passed": pass,
                "failed": fail,
                "errors": err,
                "skipped": count("skip"),
                "pass_rate": if pass + fail + err > 0 { (pass as f64 / (pass + fail + err) as f64 * 1000.0).round() / 10.0 } else { 0.0 },
                "model_calls": sum("steps"),
                "tool_calls": ran.iter().map(|r| r["tool_calls"].as_array().map_or(0, |a| a.len())).sum::<usize>(),
                "total_tokens": sum("total_tokens"),
                "prompt_tokens": sum("prompt_tokens"),
                "completion_tokens": sum("completion_tokens"),
                "cached_tokens": sum("cached_tokens"),
                "model_ms": sum("model_ms"),
                "mean_turn_s": (sum("wall_ms").saturating_sub(sum("pacing_wait_ms")) as f64 / turns as f64 / 100.0).round() / 10.0,
                "rate_limited": sum("rate_limited"),
                "rate_limit_wait_ms": sum("rate_limit_wait_ms"),
                "pacing_wait_ms": sum("pacing_wait_ms"),
                "slowest_call_ms": ran.iter().map(|r| r["slowest_call_ms"].as_u64().unwrap_or(0)).max().unwrap_or(0),
                "calls_over_20s": sum("calls_over_20s"),
                "mean_ttft_est_s": if sum("ttft_turns") > 0 {
                    json!((sum("ttft_est_ms") as f64 / sum("ttft_turns") as f64 / 100.0).round() / 10.0)
                } else {
                    Value::Null
                },
                "draft_tokens": sum("draft_tokens"),
                "draft_accepted": sum("draft_accepted"),
                "first_words": spread(&ran.iter().flat_map(|r| ms_list(&r["first_words_ms"])).collect::<Vec<_>>()),
                "turn_time": spread(&ran.iter().flat_map(|r| ms_list(&r["turn_ms"])).collect::<Vec<_>>()),
            })
        })
        .collect()
}

/// A row's list of milliseconds.
fn ms_list(v: &Value) -> Vec<u64> {
    v.as_array().into_iter().flatten().filter_map(Value::as_u64).collect()
}

/// `{n, mean_s, median_s, p90_s}` of some milliseconds (`null` for none). Pure.
fn spread(ms: &[u64]) -> Value {
    if ms.is_empty() {
        return Value::Null;
    }
    let mut v = ms.to_vec();
    v.sort_unstable();
    let s = |x: f64| (x / 10.0).round() / 100.0;
    let at = |q: f64| v[((v.len() as f64 - 1.0) * q).round() as usize] as f64;
    json!({
        "n": v.len(),
        "mean_s": s(v.iter().sum::<u64>() as f64 / v.len() as f64),
        "median_s": s(at(0.5)),
        "p90_s": s(at(0.9)),
    })
}

/// Speculative decoding's acceptance, "73% of 1.2k" ("—" without drafts).
fn acceptance(drafted: u64, accepted: u64) -> String {
    if drafted == 0 {
        return "—".to_string();
    }
    format!("{:.0}% of {}", accepted as f64 / drafted as f64 * 100.0, thousands(drafted))
}

fn thousands(n: u64) -> String {
    if n >= 10_000 {
        format!("{:.0}k", n as f64 / 1000.0)
    } else if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

fn table_only(totals: &[Value]) -> String {
    let mut md = String::from(
        "| Model | Passed | Failed | Errors | Skipped | Pass rate | Model calls | Tool calls | Tokens | Mean turn (s, excl. pacing) | First words, streamed (s, mean / median) | First words if streamed (s, est.) | Drafts accepted | Slowest call (s) | Calls > 20 s | 429s |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for t in totals {
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {}% | {} | {} | {} | {} | {} | {} | {} | {:.1} | {} | {} |\n",
            t["model"].as_str().unwrap_or(""),
            t["passed"],
            t["failed"],
            t["errors"],
            t["skipped"],
            t["pass_rate"],
            t["model_calls"],
            t["tool_calls"],
            thousands(t["total_tokens"].as_u64().unwrap_or(0)),
            t["mean_turn_s"],
            t["first_words"]["mean_s"].as_f64().map_or("—".to_string(), |m| format!(
                "{m} / {}",
                t["first_words"]["median_s"].as_f64().unwrap_or(0.0)
            )),
            t["mean_ttft_est_s"].as_f64().map_or("—".to_string(), |v| format!("{v}")),
            acceptance(t["draft_tokens"].as_u64().unwrap_or(0), t["draft_accepted"].as_u64().unwrap_or(0)),
            t["slowest_call_ms"].as_f64().unwrap_or(0.0) / 1000.0,
            t["calls_over_20s"],
            t["rate_limited"],
        ));
    }
    md
}

/// Memory's scores per model, learning (tag `memory-learn`) apart from
/// "Tell Yap what to change" / imports / remember (`memory-tell`): false
/// memories (precision), the `has` checks met (recall), and the learning
/// passes' time. "" when no scenario touched memory.
fn memory_table(rows: &[Value]) -> String {
    let with: Vec<&Value> = rows.iter().filter(|r| r.get("memory_recall_total").is_some()).collect();
    if with.is_empty() {
        return String::new();
    }
    let mut models: Vec<String> = Vec::new();
    for r in &with {
        let m = r["model"].as_str().unwrap_or("").to_string();
        if !models.contains(&m) {
            models.push(m);
        }
    }
    let mut md = String::from(
        "| Model | Group | Scenarios | False memories | Recall | Passes | Calls a pass | Mean pass (s) | Slowest pass (s) |\n|---|---|---|---|---|---|---|---|---|\n",
    );
    let mut lists = String::new();
    for m in &models {
        for (group, tag) in [("learning", "memory-learn"), ("tell / import / remember", "memory-tell"), ("other", "")] {
            let mine: Vec<&&Value> = with
                .iter()
                .filter(|r| r["model"] == json!(m))
                .filter(|r| {
                    let tags: Vec<&str> = r["tags"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                    if tag.is_empty() {
                        !tags.contains(&"memory-learn") && !tags.contains(&"memory-tell")
                    } else {
                        tags.contains(&tag)
                    }
                })
                .collect();
            if mine.is_empty() {
                continue;
            }
            let sum = |k: &str| mine.iter().map(|r| r[k].as_u64().unwrap_or(0)).sum::<u64>();
            let passes = sum("memory_passes");
            md.push_str(&format!(
                "| {m} | {group} | {} | {} | {}/{} | {} | {} | {} | {} |\n",
                mine.len(),
                sum("memory_false"),
                sum("memory_recall_hits"),
                sum("memory_recall_total"),
                passes,
                if passes > 0 { format!("{:.1}", sum("memory_pass_calls") as f64 / passes as f64) } else { "—".into() },
                if passes > 0 { format!("{:.1}", sum("memory_pass_ms") as f64 / passes as f64 / 1000.0) } else { "—".into() },
                if passes > 0 {
                    format!("{:.1}", mine.iter().map(|r| r["memory_pass_max_ms"].as_u64().unwrap_or(0)).max().unwrap_or(0) as f64 / 1000.0)
                } else {
                    "—".into()
                },
            ));
            for r in &mine {
                let texts: Vec<&str> = r["memory_false_texts"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                if !texts.is_empty() {
                    lists.push_str(&format!("- {m} · **{}**: {}\n", r["scenario"].as_str().unwrap_or(""), texts.join(" · ")));
                }
            }
        }
    }
    if !lists.is_empty() {
        md.push_str(&format!("\nFalse memories:\n\n{lists}"));
    }
    md
}

fn markdown(summary: &Value, rows: &[Value], models: &[Value], totals: &[Value]) -> String {
    let mut md = format!(
        "# Model eval {}\n\nCommit `{}` · {} → {}\n\n",
        summary["dir"].as_str().and_then(|d| Path::new(d).file_name()).map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
        summary["commit"].as_str().unwrap_or("?"),
        summary["started"].as_str().unwrap_or(""),
        summary["finished"].as_str().unwrap_or(""),
    );
    md.push_str("## Models\n\n");
    md.push_str(&table_only(totals));
    md.push('\n');
    let mem = memory_table(rows);
    if !mem.is_empty() {
        md.push_str("## Memory: precision and recall\n\n");
        md.push_str(&mem);
        md.push('\n');
    }
    for m in models {
        if let Some(e) = m["error"].as_str() {
            md.push_str(&format!("- {}: couldn't run: {e}\n", m["spec"].as_str().unwrap_or("")));
            continue;
        }
        md.push_str(&format!(
            "- **{}**: Yap's tool rule says {} (`supports_tools`); pacing {}; held back {:.0} s in all.",
            m["label"].as_str().unwrap_or(""),
            if m["tools_rule"] == json!(true) { "tools" } else { "no tools (plain chat + RAG)" },
            m["tokens_per_minute"].as_u64().map_or("none".to_string(), |t| format!("{t} tokens/min")),
            m["paced_ms"].as_f64().unwrap_or(0.0) / 1000.0,
        ));
        if m["gguf"].is_string() {
            md.push_str(&format!(
                " GGUF {} MB, loaded in {:.1} s; memory: llama.cpp projects {} MB (model + context), peak working set {} MB (its prompt cache reached {} MB); prompt {} tok/s, generation {} tok/s.",
                m["gguf_mb"],
                m["load_ms"].as_f64().unwrap_or(0.0) / 1000.0,
                m["log_facts"]["projected_host_mib"],
                m["peak_ram_mb"],
                m["log_facts"]["prompt_cache_peak_mib"],
                m["prompt_tokens_per_s"],
                m["gen_tokens_per_s"],
            ));
        }
        md.push('\n');
    }
    let models_order: Vec<String> = totals.iter().filter_map(|t| t["model"].as_str().map(str::to_string)).collect();
    let mut scen_order: Vec<String> = Vec::new();
    for r in rows {
        let s = r["scenario"].as_str().unwrap_or("").to_string();
        if !scen_order.contains(&s) {
            scen_order.push(s);
        }
    }
    md.push_str("\n## Scenarios\n\n| Scenario |");
    for m in &models_order {
        md.push_str(&format!(" {m} |"));
    }
    md.push_str("\n|---|");
    for _ in &models_order {
        md.push_str("---|");
    }
    md.push('\n');
    for s in &scen_order {
        md.push_str(&format!("| {s} |"));
        for m in &models_order {
            let cell = rows
                .iter()
                .find(|r| r["scenario"] == json!(s) && r["model"] == json!(m))
                .map(|r| match r["status"].as_str() {
                    Some("pass") | Some("fail") => format!(
                        "{} · {} calls · {} tok · {:.1} s{}",
                        if r["status"] == "pass" { "PASS" } else { "**FAIL**" },
                        r["steps"],
                        thousands(r["total_tokens"].as_u64().unwrap_or(0)),
                        (r["wall_ms"].as_u64().unwrap_or(0).saturating_sub(r["pacing_wait_ms"].as_u64().unwrap_or(0))) as f64 / 1000.0,
                        if r["warnings"].as_array().is_some_and(|w| !w.is_empty()) { " (warn)" } else { "" }
                    ),
                    Some("skip") => "skip".to_string(),
                    _ => "**ERROR**".to_string(),
                })
                .unwrap_or_default();
            md.push_str(&format!(" {cell} |"));
        }
        md.push('\n');
    }
    for (title, key, statuses) in [
        ("Failures", "reasons", &["fail", "error"][..]),
        ("Warnings (soft checks)", "warnings", &["pass", "fail"][..]),
    ] {
        let mut section = String::new();
        for m in &models_order {
            let mut lines = String::new();
            for r in rows.iter().filter(|r| r["model"] == json!(m) && statuses.iter().any(|s| r["status"] == json!(s))) {
                let items: Vec<&str> = r[key].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                if !items.is_empty() {
                    lines.push_str(&format!("- **{}**: {}\n", r["scenario"].as_str().unwrap_or(""), items.join("; ")));
                }
            }
            if !lines.is_empty() {
                section.push_str(&format!("\n### {m}\n\n{lines}"));
            }
        }
        if !section.is_empty() {
            md.push_str(&format!("\n## {title}\n{section}"));
        }
    }
    md.push_str("\n## Answers\n");
    for m in &models_order {
        md.push_str(&format!("\n### {m}\n\n"));
        for r in rows.iter().filter(|r| r["model"] == json!(m) && r["turn_details"].is_array()) {
            for (i, t) in r["turn_details"].as_array().into_iter().flatten().enumerate() {
                let reply = t["reply"].as_str().unwrap_or("");
                let reply = if reply.is_empty() { t["error"].as_str().map(|e| format!("(error: {e})")).unwrap_or_default() } else { reply.to_string() };
                let reply: String = reply.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(400).collect();
                let tools: Vec<String> = t["stats"]["tool_calls"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| format!("{}({})", c["name"].as_str().unwrap_or(""), c["args"].to_string().chars().take(60).collect::<String>()))
                    .collect();
                md.push_str(&format!(
                    "- **{}**{} \"{}\" → {}{}\n",
                    r["scenario"].as_str().unwrap_or(""),
                    if r["turns"].as_u64().unwrap_or(1) > 1 { format!(" [{}]", i + 1) } else { String::new() },
                    t["user"].as_str().unwrap_or(""),
                    if t["fast_path"] == json!(true) { "(Yap) ".to_string() } else if tools.is_empty() { String::new() } else { format!("[{}] ", tools.join(", ")) },
                    reply
                ));
            }
        }
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_parse() {
        let a = parse(
            ["--model", "groq:openai/gpt-oss-20b", "--model=local:installed", "--only", "apps,notes", "--tools", "on", "--tpm", "7000", "--ngl", "0"]
                .map(String::from)
                .to_vec(),
        )
        .unwrap();
        assert_eq!(a.models, ["groq:openai/gpt-oss-20b", "local:installed"]);
        assert_eq!(a.only, ["apps", "notes"]);
        assert_eq!(a.tools, Some(true));
        assert_eq!(a.tokens_per_minute, Some(7000));
        assert_eq!(a.ctx, 8192);
        assert!(parse(vec!["--bogus".into()]).is_err());
        let b = parse(
            ["--bench", "stream", "--temperature", "0", "--set", "id_slot=2", "--set=cache_prompt=false", "--cleanup-slot", "0"]
                .map(String::from)
                .to_vec(),
        )
        .unwrap();
        assert_eq!(b.bench.as_deref(), Some("stream"));
        assert_eq!(b.cleanup_slot, Some(0));
        let body = run_body(&b);
        assert_eq!(Value::Object(body), json!({ "temperature": 0.0, "id_slot": 2, "cache_prompt": false }));
        assert!(parse(["--bench", "nope"].map(String::from).to_vec()).is_err());
        assert_eq!(ttft_estimate(5_000, 2_000, 350.4), 3_350);
        assert_eq!(acceptance(0, 0), "—");
        assert_eq!(acceptance(200, 150), "75% of 200");
        assert!(parse(vec!["--model".into()]).is_err());
    }

    #[test]
    fn names_for_folders() {
        assert_eq!(model_short("groq:openai/gpt-oss-20b"), "gpt-oss-20b");
        assert_eq!(model_short(r"local:E:\m\Qwen2.5-3B-Instruct-Q4_K_M.gguf"), "Qwen2.5-3B-Instruct-Q4_K_M");
        assert_eq!(model_short("custom:http://localhost:11434/v1,llama3.2"), "llama3.2");
        assert_eq!(slug("gpt-oss-20b+Qwen2.5 3B"), "gpt-oss-20b-qwen2.5-3b");
    }

    /// Every scenario in the repo parses (unknown fields are errors).
    #[test]
    fn the_repos_scenarios_parse() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("evals").join("scenarios");
        let args = Args { scenarios: Some(dir), ..Default::default() };
        let all = load_scenarios(&args).expect("scenarios load");
        assert!(all.len() >= 12, "{} scenarios", all.len());
        for s in &all {
            assert!(!s.turns.is_empty(), "{} has no turns", s.name);
            if let Apps::Named(n) = &s.apps {
                if n != "this_pc" {
                    let mut none = None;
                    resolve_apps(&s.apps, &scenarios_dir(&args), Path::new("."), &mut none).expect("its apps load");
                }
            }
        }
    }
}

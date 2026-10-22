//! `--cleanup`: **dictation cleanup latency** on each `--model`, through
//! the cleanup's own code (`llm::cleanup`: the guardrails, Settings' default
//! body, the three one-shots, the transcript framed as data) — the call a
//! dictation waits on between the speech-to-text and the paste.
//!
//! Each dictation in `evals/cleanup/dictations.json` (typical one-to-three
//! sentence dictations, as speech-to-text writes them) is cleaned:
//!
//! - **back to back** (`rounds` times): one dictation after another, the
//!   server's own cache warm with the cleanup prompt — the common case;
//! - **after a Chat turn**: a Chat-shaped request (the persona, the tool
//!   instructions and the tools' schemas, ~2k tokens) goes first, untimed,
//!   so the cleanup finds the server busy with something else's prompt —
//!   what a dictation right after asking Chat something gets.
//!
//! Per phase: mean, median, 90th percentile and slowest wall time, and
//! llama.cpp's own counts (prompt tokens it had to process — the rest came
//! from its cache — and tokens generated). The first call after the model
//! loaded is reported apart. Every output is kept, to compare the cleanup
//! itself.

use std::path::Path;
use std::time::Instant;

use serde_json::{json, Value};

use super::record;

/// The dictations and how they're used.
#[derive(Debug, serde::Deserialize)]
pub struct Bench {
    pub dictations: Vec<String>,
    /// Dictations with words English spells two ways, as speech-to-text
    /// writes them (American): does cleanup follow "Use my region's
    /// spelling" (`about_you::cleanup_spelling`, the run's `--locale`)?
    #[serde(default)]
    pub spelling: Vec<SpellingCase>,
}

/// One spelling dictation and its words, each way.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SpellingCase {
    pub text: String,
    pub british: Vec<String>,
    pub american: Vec<String>,
}

/// How many of `words` an output has (case-blind, whole words). Pure.
fn count_words(output: &str, words: &[String]) -> usize {
    let lower = output.to_lowercase();
    let tokens: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).collect();
    words.iter().filter(|w| tokens.contains(&w.to_lowercase().as_str())).count()
}

pub fn load(path: &Path) -> Result<Bench, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let b: Bench = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if b.dictations.is_empty() {
        return Err(format!("{}: no dictations", path.display()));
    }
    Ok(b)
}

/// One timed cleanup.
#[derive(Debug, Clone, serde::Serialize)]
struct Shot {
    phase: &'static str,
    input: String,
    output: String,
    error: Option<String>,
    ms: u64,
    prompt_tokens: u64,
    processed_tokens: u64,
    gen_tokens: u64,
    prompt_ms: f64,
    gen_ms: f64,
    draft_tokens: u64,
    draft_accepted: u64,
}

/// How the bench sends the cleanup: `--cleanup-slot` / `--chat-slot`, the
/// llama.cpp slots (`id_slot`) the cleanup and the Chat turns between them
/// go to (`None`: the server picks, by prompt similarity, else the least
/// recently used), and `--cleanup-layout`, the same prompt laid out another
/// way ([`LAYOUTS`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct Setup {
    pub cleanup: Option<u64>,
    pub chat: Option<u64>,
    pub layout: Option<&'static str>,
}

/// `--cleanup-layout`: `split` (the restated instruction as a user message
/// of its own, the transcript after it) or `system` (the instruction at the
/// end of the system prompt; every user message only "Transcript: …").
/// Today the last user message carries the instruction (~370 tokens) before
/// the transcript, and a hybrid model like Qwen3.5 re-uses its cache only up
/// to a checkpoint before that message, so it reads the instruction again
/// for every dictation.
pub const LAYOUTS: &[&str] = &["split", "system"];

const TRANSCRIPT_MARK: &str = "\n\nTranscript:\n";

/// The cleanup's last user message as two: the instruction, then the
/// transcript.
fn split_layout(body: &mut Value) {
    let Some(msgs) = body["messages"].as_array_mut() else { return };
    let Some(last) = msgs.last().and_then(|m| m["content"].as_str()).map(str::to_string) else { return };
    if let Some((instruction, rest)) = last.split_once(TRANSCRIPT_MARK) {
        msgs.pop();
        msgs.push(json!({ "role": "user", "content": instruction }));
        msgs.push(json!({ "role": "user", "content": format!("Transcript:\n{rest}") }));
    }
}

/// The instruction moved to the system prompt; each user message keeps only
/// its transcript.
fn system_layout(body: &mut Value) {
    let Some(msgs) = body["messages"].as_array_mut() else { return };
    let mut instruction = None;
    for m in msgs.iter_mut().filter(|m| m["role"] == "user") {
        let Some(text) = m["content"].as_str().map(str::to_string) else { continue };
        if let Some((ins, rest)) = text.split_once(TRANSCRIPT_MARK) {
            instruction.get_or_insert_with(|| ins.to_string());
            m["content"] = json!(format!("Transcript:\n{rest}"));
        }
    }
    if let (Some(ins), Some(sys)) = (instruction, msgs.first_mut().filter(|m| m["role"] == "system")) {
        let s = sys["content"].as_str().unwrap_or("").to_string();
        sys["content"] = json!(format!("{s}\n\n{ins}"));
    }
}

/// Send this phase's requests to `slot` (or wherever the server likes).
fn pin(slot: Option<u64>) {
    let mut m = serde_json::Map::new();
    if let Some(n) = slot {
        m.insert("id_slot".into(), json!(n));
    }
    record::set_phase_body(m);
}

async fn clean(phase: &'static str, text: &str, ep: &Endpoint<'_>, setup: Setup) -> Shot {
    let cfg = crate::config::YapConfig::default();
    // "Use my region's spelling" (on by default) for the run's locale
    // (`--locale`; none = no spelling line, as before 2026-10-21).
    let spelling = crate::about_you::cleanup_spelling(&cfg);
    pin(setup.cleanup);
    record::set_phase_rewrite(match setup.layout {
        Some("split") => Some(split_layout as fn(&mut Value)),
        Some("system") => Some(system_layout as fn(&mut Value)),
        _ => None,
    });
    record::begin_turn();
    let t0 = Instant::now();
    let out = crate::llm::cleanup(text, ep.base_url, ep.key, ep.model, ep.provider, &cfg.pp_prompt, spelling.as_deref(), &[], false).await;
    let ms = t0.elapsed().as_millis() as u64;
    record::set_phase_rewrite(None);
    let st = record::end_turn();
    let (output, error) = match out {
        Ok(o) => (o, None),
        Err(e) => (String::new(), Some(e)),
    };
    Shot {
        phase,
        input: text.to_string(),
        output,
        error,
        ms,
        prompt_tokens: st.prompt_tokens,
        processed_tokens: st.prompt_eval_tokens,
        gen_tokens: st.gen_tokens,
        prompt_ms: st.prompt_eval_ms,
        gen_ms: st.gen_ms,
        draft_tokens: st.draft_tokens,
        draft_accepted: st.draft_accepted,
    }
}

/// A Chat turn's first request (persona + tool instructions + the tools),
/// untimed: what else the server was doing just before a dictation. Whether
/// it was answered.
async fn chat_between(question: &str, ep: &Endpoint<'_>, setup: Setup) -> bool {
    let mut system = format!("{}\n\nYou have access to tools. ", super::runner::DEFAULT_PERSONA);
    system.push_str(&crate::tools::TOOL_INSTRUCTIONS.join(" "));
    system.push(' ');
    system.push_str(&crate::tools::APP_TOOL_INSTRUCTIONS.join(" "));
    let mut tools = crate::tools::definitions();
    if let (Some(all), Some(apps)) = (tools.as_array_mut(), crate::tools::app_definitions().as_array()) {
        all.extend(apps.iter().cloned());
    }
    let messages = json!([{ "role": "system", "content": system }, { "role": "user", "content": question }]);
    pin(setup.chat);
    record::begin_turn();
    let ok = crate::llm::post_chat_message(ep.base_url, ep.key, ep.model, ep.provider, 0.5, messages, Some(&tools), None).await.is_ok();
    let _ = record::end_turn();
    pin(None);
    ok
}

/// A long Chat question (~6k tokens with the tools: a long note pasted in),
/// which fills most of the server's 8k context next to the cleanup's prompt.
fn long_question() -> String {
    let para = "The quarterly planning meeting covered the launch schedule, the hiring plan for the support team, the budget for the new office in Leeds, and the migration of the billing system. Dave said the beta could ship on Friday if the installer bug is fixed by Wednesday; Priya will check the signing certificate. ";
    format!("Summarise this note in three bullets:\n\n{}", para.repeat(60))
}

pub struct Endpoint<'a> {
    pub base_url: &'a str,
    pub key: &'a str,
    pub model: &'a str,
    pub provider: &'a str,
}

const CHAT_QUESTIONS: &[&str] = &[
    "What did we decide about the launch date?",
    "Open Spotify",
    "What's on my shopping list?",
    "Summarise my last meeting in two lines.",
];

/// Run the bench on one model; its results (JSON) for summary.json and
/// cleanup.md.
pub async fn run(bench: &Bench, ep: &Endpoint<'_>, rounds: u32, setup: Setup) -> Value {
    let mut shots: Vec<Shot> = Vec::new();
    // The first call after the model loaded (its caches cold).
    let first = clean("first", &bench.dictations[0], ep, setup).await;
    println!("  first cleanup after loading: {} ms", first.ms);
    shots.push(first);
    for _ in 0..rounds.max(1) {
        for d in &bench.dictations {
            let s = clean("back to back", d, ep, setup).await;
            shots.push(s);
        }
    }
    let mut chat_failed = 0;
    for (i, d) in bench.dictations.iter().enumerate() {
        if !chat_between(CHAT_QUESTIONS[i % CHAT_QUESTIONS.len()], ep, setup).await {
            chat_failed += 1;
        }
        let s = clean("after a Chat turn", d, ep, setup).await;
        shots.push(s);
    }
    // A long Chat turn (most of the context) between dictations: does it
    // still fit beside the cleanup's prompt, and what does the next cleanup
    // cost?
    let long = long_question();
    let mut long_failed = 0;
    for d in bench.dictations.iter().take(4) {
        if !chat_between(&long, ep, setup).await {
            long_failed += 1;
        }
        let s = clean("after a long Chat turn", d, ep, setup).await;
        shots.push(s);
    }
    pin(None);
    println!("  Chat turns that failed: {chat_failed} of {}, long ones {long_failed} of 4", bench.dictations.len());
    // The spelling dictations, `rounds` times each: which way the words
    // come out.
    let mut spelling_outputs: Vec<Value> = Vec::new();
    let (mut british, mut american, mut words) = (0usize, 0usize, 0usize);
    for _ in 0..rounds.max(1) {
        for case in &bench.spelling {
            let s = clean("spelling", &case.text, ep, setup).await;
            let (b, a) = (count_words(&s.output, &case.british), count_words(&s.output, &case.american));
            british += b;
            american += a;
            words += case.british.len();
            spelling_outputs.push(json!({ "input": s.input, "output": s.output, "error": s.error, "ms": s.ms, "british": b, "american": a, "of": case.british.len() }));
            shots.push(s);
        }
    }
    let spelling = json!({
        "line": crate::about_you::cleanup_spelling(&crate::config::YapConfig::default()),
        "british": british,
        "american": american,
        "words": words,
        "outputs": spelling_outputs,
    });
    if words > 0 {
        println!("  spelling: {british} of {words} words British, {american} American");
    }
    let phases: Vec<Value> = ["first", "back to back", "after a Chat turn", "after a long Chat turn", "spelling"]
        .iter()
        .map(|p| stats(p, &shots.iter().filter(|s| s.phase == *p).collect::<Vec<_>>()))
        .collect();
    for p in &phases {
        println!(
            "  {:<22} n={:<3} mean {:>5} ms  median {:>5}  p90 {:>5}  max {:>5}  processed {:>5} tok  gen {:>3} tok ({} tok/s)  drafts kept {}  errors {}",
            p["phase"].as_str().unwrap_or(""),
            p["n"],
            p["mean_ms"],
            p["median_ms"],
            p["p90_ms"],
            p["max_ms"],
            p["mean_processed_tokens"],
            p["mean_gen_tokens"],
            p["gen_tokens_per_s"],
            p["draft_acceptance"],
            p["errors"],
        );
    }
    let outputs: Vec<Value> = shots
        .iter()
        .filter(|s| s.phase == "back to back")
        .take(bench.dictations.len())
        .map(|s| json!({ "input": s.input, "output": s.output, "error": s.error, "ms": s.ms }))
        .collect();
    // Every back-to-back output, in order: at --temperature 0 two runs (a
    // draft model or not) should write the same text.
    let all_outputs: Vec<&str> = shots.iter().filter(|s| s.phase == "back to back").map(|s| s.output.as_str()).collect();
    json!({
        "phases": phases,
        "chat_failed": chat_failed,
        "long_chat_failed": long_failed,
        "outputs": outputs,
        "spelling": spelling,
        "all_outputs": all_outputs,
        "shots": shots,
    })
}

fn stats(phase: &str, shots: &[&Shot]) -> Value {
    let mut ms: Vec<u64> = shots.iter().map(|s| s.ms).collect();
    ms.sort_unstable();
    let n = ms.len().max(1);
    let pick = |q: f64| ms.get(((ms.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or(0);
    let mean = |f: &dyn Fn(&Shot) -> u64| shots.iter().map(|s| f(s)).sum::<u64>() / n as u64;
    json!({
        "phase": phase,
        "n": shots.len(),
        "mean_ms": ms.iter().sum::<u64>() / n as u64,
        "median_ms": pick(0.5),
        "p90_ms": pick(0.9),
        "max_ms": ms.last().copied().unwrap_or(0),
        "mean_prompt_tokens": mean(&|s| s.prompt_tokens),
        "mean_processed_tokens": mean(&|s| s.processed_tokens),
        "mean_gen_tokens": mean(&|s| s.gen_tokens),
        "mean_prompt_ms": (shots.iter().map(|s| s.prompt_ms).sum::<f64>() / n as f64).round(),
        "mean_gen_ms": (shots.iter().map(|s| s.gen_ms).sum::<f64>() / n as f64).round(),
        "gen_tokens_per_s": rate(shots.iter().map(|s| s.gen_tokens).sum::<u64>(), shots.iter().map(|s| s.gen_ms).sum::<f64>()),
        "draft_acceptance": share(
            shots.iter().map(|s| s.draft_accepted).sum::<u64>(),
            shots.iter().map(|s| s.draft_tokens).sum::<u64>(),
        ),
        "errors": shots.iter().filter(|s| s.error.is_some()).count(),
    })
}

/// Tokens a second (`null` without timings). Pure.
fn rate(tokens: u64, ms: f64) -> Value {
    if ms > 0.0 {
        json!((tokens as f64 / ms * 1000.0 * 10.0).round() / 10.0)
    } else {
        Value::Null
    }
}

/// `part / whole`, 3 decimals (`null` when nothing was drafted). Pure.
fn share(part: u64, whole: u64) -> Value {
    if whole > 0 {
        json!((part as f64 / whole as f64 * 1000.0).round() / 1000.0)
    } else {
        Value::Null
    }
}

/// cleanup.md: the phases per model, then the outputs side by side.
pub fn markdown(models: &[(String, Value)]) -> String {
    let mut md = String::from("# Dictation cleanup latency\n\nWall time of `llm::cleanup` per dictation (ms), llama.cpp's prompt tokens processed (the rest came from its cache) and generated, its prompt and generation time (ms), and speculative decoding's share of drafted tokens kept.\n\n| Model | Phase | n | Mean | Median | p90 | Max | Prompt tok processed | Prompt ms | Gen tok | Gen ms | Gen tok/s | Drafts kept | Errors |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n");
    for (label, r) in models {
        for p in r["phases"].as_array().into_iter().flatten() {
            md.push_str(&format!(
                "| {label} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                p["phase"].as_str().unwrap_or(""),
                p["n"],
                p["mean_ms"],
                p["median_ms"],
                p["p90_ms"],
                p["max_ms"],
                p["mean_processed_tokens"],
                p["mean_prompt_ms"],
                p["mean_gen_tokens"],
                p["mean_gen_ms"],
                p["gen_tokens_per_s"],
                p["draft_acceptance"].as_f64().map_or("—".to_string(), |a| format!("{:.0}%", a * 100.0)),
                p["errors"]
            ));
        }
    }
    md.push_str("\n## Outputs\n");
    for (label, r) in models {
        md.push_str(&format!(
            "\n### {label}\n\nChat turns between the dictations that failed: {}, long ones {} of 4.\n\n",
            r["chat_failed"], r["long_chat_failed"]
        ));
        for o in r["outputs"].as_array().into_iter().flatten() {
            let out = o["output"].as_str().filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| {
                format!("(error: {})", o["error"].as_str().unwrap_or("none"))
            });
            md.push_str(&format!("- \"{}\" → \"{}\" ({} ms)\n", o["input"].as_str().unwrap_or(""), out.replace('\n', " / "), o["ms"]));
        }
        let sp = &r["spelling"];
        if sp["words"].as_u64().unwrap_or(0) > 0 {
            md.push_str(&format!(
                "\n**Spelling** ({}): {} of {} words British, {} American.\n\n",
                sp["line"].as_str().unwrap_or("no spelling line"),
                sp["british"],
                sp["words"],
                sp["american"]
            ));
            for o in sp["outputs"].as_array().into_iter().flatten() {
                md.push_str(&format!(
                    "- \"{}\" → \"{}\" ({} of {} British, {} American; {} ms)\n",
                    o["input"].as_str().unwrap_or(""),
                    o["output"].as_str().unwrap_or("").replace('\n', " / "),
                    o["british"],
                    o["of"],
                    o["american"],
                    o["ms"]
                ));
            }
        }
    }
    md
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_dictations_parse() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/cleanup/dictations.json");
        let b = super::load(&dir).expect("dictations.json");
        assert!(b.dictations.len() >= 8);
    }

    #[test]
    fn the_layouts_keep_the_words() {
        let body = || {
            serde_json::json!({ "messages": [
                { "role": "system", "content": "Guardrails." },
                { "role": "user", "content": "Clean it.\n\nTranscript:\n\"\"\"\num hi\n\"\"\"" },
                { "role": "assistant", "content": "Hi." },
                { "role": "user", "content": "Clean it.\n\nTranscript:\n\"\"\"\nso yeah\n\"\"\"" },
            ] })
        };
        let mut b = body();
        super::split_layout(&mut b);
        let m = b["messages"].as_array().unwrap();
        assert_eq!(m.len(), 5);
        assert_eq!(m[3]["content"], "Clean it.");
        assert_eq!(m[4]["content"], "Transcript:\n\"\"\"\nso yeah\n\"\"\"");
        let mut b = body();
        super::system_layout(&mut b);
        let m = b["messages"].as_array().unwrap();
        assert_eq!(m.len(), 4);
        assert_eq!(m[0]["content"], "Guardrails.\n\nClean it.");
        assert_eq!(m[1]["content"], "Transcript:\n\"\"\"\num hi\n\"\"\"");
        assert_eq!(m[3]["content"], "Transcript:\n\"\"\"\nso yeah\n\"\"\"");
    }

    #[test]
    fn rates_and_shares() {
        assert_eq!(super::rate(100, 1000.0), serde_json::json!(100.0));
        assert!(super::rate(5, 0.0).is_null());
        assert_eq!(super::share(3, 4), serde_json::json!(0.75));
        assert!(super::share(0, 0).is_null());
    }
}

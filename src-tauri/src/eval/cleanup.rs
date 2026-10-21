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
}

async fn clean(phase: &'static str, text: &str, ep: &Endpoint<'_>) -> Shot {
    let body = crate::config::YapConfig::default().pp_prompt;
    record::begin_turn();
    let t0 = Instant::now();
    let out = crate::llm::cleanup(text, ep.base_url, ep.key, ep.model, ep.provider, &body, &[], false).await;
    let ms = t0.elapsed().as_millis() as u64;
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
    }
}

/// A Chat turn's first request (persona + tool instructions + the tools),
/// untimed: what else the server was doing just before a dictation. Whether
/// it was answered.
async fn chat_between(question: &str, ep: &Endpoint<'_>) -> bool {
    let mut system = format!("{}\n\nYou have access to tools. ", super::runner::DEFAULT_PERSONA);
    system.push_str(&crate::tools::TOOL_INSTRUCTIONS.join(" "));
    system.push(' ');
    system.push_str(&crate::tools::APP_TOOL_INSTRUCTIONS.join(" "));
    let mut tools = crate::tools::definitions();
    if let (Some(all), Some(apps)) = (tools.as_array_mut(), crate::tools::app_definitions().as_array()) {
        all.extend(apps.iter().cloned());
    }
    let messages = json!([{ "role": "system", "content": system }, { "role": "user", "content": question }]);
    record::begin_turn();
    let ok = crate::llm::post_chat_message(ep.base_url, ep.key, ep.model, ep.provider, 0.5, messages, Some(&tools), None).await.is_ok();
    let _ = record::end_turn();
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
pub async fn run(bench: &Bench, ep: &Endpoint<'_>, rounds: u32) -> Value {
    let mut shots: Vec<Shot> = Vec::new();
    // The first call after the model loaded (its caches cold).
    let first = clean("first", &bench.dictations[0], ep).await;
    println!("  first cleanup after loading: {} ms", first.ms);
    shots.push(first);
    for _ in 0..rounds.max(1) {
        for d in &bench.dictations {
            let s = clean("back to back", d, ep).await;
            shots.push(s);
        }
    }
    let mut chat_failed = 0;
    for (i, d) in bench.dictations.iter().enumerate() {
        if !chat_between(CHAT_QUESTIONS[i % CHAT_QUESTIONS.len()], ep).await {
            chat_failed += 1;
        }
        let s = clean("after a Chat turn", d, ep).await;
        shots.push(s);
    }
    // A long Chat turn (most of the context) between dictations: does it
    // still fit beside the cleanup's prompt, and what does the next cleanup
    // cost?
    let long = long_question();
    let mut long_failed = 0;
    for d in bench.dictations.iter().take(4) {
        if !chat_between(&long, ep).await {
            long_failed += 1;
        }
        let s = clean("after a long Chat turn", d, ep).await;
        shots.push(s);
    }
    println!("  Chat turns that failed: {chat_failed} of {}, long ones {long_failed} of 4", bench.dictations.len());
    let phases: Vec<Value> = ["first", "back to back", "after a Chat turn", "after a long Chat turn"]
        .iter()
        .map(|p| stats(p, &shots.iter().filter(|s| s.phase == *p).collect::<Vec<_>>()))
        .collect();
    for p in &phases {
        println!(
            "  {:<22} n={:<3} mean {:>5} ms  median {:>5}  p90 {:>5}  max {:>5}  processed {:>5} tok  gen {:>3} tok  errors {}",
            p["phase"].as_str().unwrap_or(""),
            p["n"],
            p["mean_ms"],
            p["median_ms"],
            p["p90_ms"],
            p["max_ms"],
            p["mean_processed_tokens"],
            p["mean_gen_tokens"],
            p["errors"],
        );
    }
    let outputs: Vec<Value> = shots
        .iter()
        .filter(|s| s.phase == "back to back")
        .take(bench.dictations.len())
        .map(|s| json!({ "input": s.input, "output": s.output, "error": s.error, "ms": s.ms }))
        .collect();
    json!({
        "phases": phases,
        "chat_failed": chat_failed,
        "long_chat_failed": long_failed,
        "outputs": outputs,
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
        "errors": shots.iter().filter(|s| s.error.is_some()).count(),
    })
}

/// cleanup.md: the phases per model, then the outputs side by side.
pub fn markdown(models: &[(String, Value)]) -> String {
    let mut md = String::from("# Dictation cleanup latency\n\nWall time of `llm::cleanup` per dictation (ms), and llama.cpp's prompt tokens processed (the rest came from its cache) and generated.\n\n| Model | Phase | n | Mean | Median | p90 | Max | Prompt tok processed | Gen tok | Errors |\n|---|---|---|---|---|---|---|---|---|---|\n");
    for (label, r) in models {
        for p in r["phases"].as_array().into_iter().flatten() {
            md.push_str(&format!(
                "| {label} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                p["phase"].as_str().unwrap_or(""),
                p["n"],
                p["mean_ms"],
                p["median_ms"],
                p["p90_ms"],
                p["max_ms"],
                p["mean_processed_tokens"],
                p["mean_gen_tokens"],
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
}

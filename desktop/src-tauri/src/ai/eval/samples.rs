//! `--samples`: **writing samples** (`ai/writing_samples.rs`) on each
//! `--model`, through the app's own calls, over `evals/samples/cases.json`:
//! a made-up person's samples (their voice: British spelling, "Hiya all,",
//! "Cheers, Robin", dashes, no exclamation marks, lower-case chats) and
//! texts to rewrite — a transform (`llm::transform_with_samples`, the
//! built-in's instruction), the rewrite shortcut writing new text
//! (`llm::rewrite_with_samples`), and AI cleanup at High
//! (`cleanup_styles::clean` with the plan's samples) — each **without**
//! samples (what every call sent until 29 Oct 2026) and **with** them (the
//! block for the case's kind of app, sized for where the model is:
//! `writing_samples::budget_for`, or `YAP_EVAL_SAMPLES_BUDGET` characters).
//!
//! Judged on:
//! - **voice**: at least one of the case's `voice` markers (what the person
//!   writes: "Cheers", "Hiya") and none of its `generic` ones (what a model
//!   writes that they never do: "Best regards", "!");
//! - **kept**: the case's `expect` (facts kept, nothing answered:
//!   `transforms::judge`);
//! - **no leaks**: none of the samples' own facts (`leaks`: "Harbour
//!   Street", "allotment") in a text that didn't have them.
//!
//! Times and tokens per phase: the wall time, the prompt's tokens and
//! llama.cpp's prompt tokens processed (the rest came from its cache).
//! `samples.md` in the run folder; `models[].samples` in summary.json.

use std::path::Path;
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};

use super::cleanup::Endpoint;
use super::record;
use crate::ai::cleanup_styles::{self, Category};
use crate::ai::writing_samples::{self, Sample};

pub const PHASES: &[&str] = &["without", "with"];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bench {
    /// The person's samples, as Settings keeps them.
    pub samples: Vec<SampleIn>,
    /// Facts only the samples hold: never in a result whose text lacked them.
    pub leaks: Vec<String>,
    pub cases: Vec<Case>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleIn {
    #[serde(default)]
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    /// "transform" | "rewrite" | "cleanup".
    pub call: String,
    /// A transform's built-in kind ("polish", "shorter").
    #[serde(default)]
    pub transform: String,
    /// The rewrite shortcut's spoken instruction.
    #[serde(default)]
    pub instruction: String,
    /// The kind of app (`cleanup_styles` category id).
    pub app: String,
    /// The text to transform or clean (a rewrite's selection; "" writes new).
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub expect: super::transforms::Expect,
    /// What the person writes (one at least, case-insensitive).
    #[serde(default)]
    pub voice: Vec<String>,
    /// What a model writes that the person never does.
    #[serde(default)]
    pub generic: Vec<String>,
}

pub fn load(path: &Path, only: &[String]) -> Result<Bench, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    let mut b: Bench = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let only: Vec<String> = only.iter().map(|s| s.to_lowercase()).collect();
    b.cases.retain(|c| only.is_empty() || only.iter().any(|o| c.name.to_lowercase().contains(o) || c.call == *o));
    if b.cases.is_empty() {
        return Err(format!("no writing-sample cases in {}", path.display()));
    }
    Ok(b)
}

fn samples(b: &Bench) -> Vec<Sample> {
    b.samples
        .iter()
        .enumerate()
        .map(|(i, s)| Sample { id: format!("s-{i}"), text: s.text.clone(), kind: s.kind.clone(), added_ms: 0 })
        .collect()
}

fn category(c: &Case) -> Category {
    Category::from_id(&c.app).unwrap_or(Category::Other)
}

/// The block a case's call carries in `phase` (none without).
pub fn block_for(b: &Bench, c: &Case, phase: &str, base_url: &str) -> Option<String> {
    if phase != "with" {
        return None;
    }
    let budget = std::env::var("YAP_EVAL_SAMPLES_BUDGET")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or_else(|| writing_samples::budget_for(base_url));
    let kind = writing_samples::kind_for(category(c))?;
    writing_samples::block(&writing_samples::pick(&samples(b), kind, budget))
}

/// One case, once: the text as the app would paste it, and an error.
async fn ask(b: &Bench, c: &Case, phase: &str, ep: &Endpoint<'_>) -> (String, Option<String>) {
    let block = block_for(b, c, phase, ep.base_url);
    let asked = match c.call.as_str() {
        "transform" => {
            let instruction = crate::dictation::transforms::builtin_instruction(&c.transform, "").unwrap_or_default();
            crate::ai::llm::transform_with_samples(&instruction, &c.text, block.as_deref(), ep.base_url, ep.key, ep.model, ep.provider, false)
                .await
                .map(|o| crate::dictation::transforms::clean_output(&o))
        }
        "rewrite" => crate::ai::llm::rewrite_with_samples(&c.instruction, &c.text, block.as_deref(), ep.base_url, ep.key, ep.model, ep.provider, "", false).await,
        _ => {
            let mut cfg = crate::config::YapConfig::default();
            cleanup_styles::on_load(&mut cfg);
            cfg.cleanup_strength = "high".into();
            match cleanup_styles::resolve(&cfg, &cleanup_styles::sample_target(category(c))) {
                Some(mut plan) => {
                    // `YAP_EVAL_SAMPLES_TONE=natural`: with the samples, the
                    // kind's tone sentence left out (the samples are the voice).
                    if block.is_some() && std::env::var("YAP_EVAL_SAMPLES_TONE").as_deref() == Ok("natural") {
                        plan.style = plan.style.map(|s| cleanup_styles::Style { tone: cleanup_styles::Tone::Natural, ..s });
                    }
                    plan.samples = block;
                    let spelling = crate::ai::about_you::cleanup_spelling(&cfg);
                    cleanup_styles::clean(&c.text, ep.base_url, ep.key, ep.model, ep.provider, &plan, spelling.as_deref(), None, &[], false).await
                }
                None => Ok(c.text.clone()),
            }
        }
    };
    match asked {
        Ok(out) => (out.trim().to_string(), None),
        Err(e) => (String::new(), Some(record::redact(&e))),
    }
}

/// `needle` in `text`, case-insensitive (a word of letters whole);
/// "<lower>": the text starts with a small letter.
fn has(text: &str, needle: &str) -> bool {
    if needle == "<lower>" {
        return text.chars().find(|c| c.is_alphabetic()).is_some_and(char::is_lowercase);
    }
    let t = text.to_lowercase();
    let n = needle.to_lowercase();
    if n.chars().all(|c| c.is_alphanumeric() || c == ' ' || c == '\'') {
        t.match_indices(&n).any(|(i, _)| {
            let before = t[..i].chars().next_back();
            let after = t[i + n.len()..].chars().next();
            !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
        })
    } else {
        t.contains(&n)
    }
}

/// How a result reads (pure): whether it sounds like the person, what it
/// failed of the case, and the samples' facts it took.
pub fn judge(b: &Bench, c: &Case, out: &str) -> (bool, Vec<String>, Vec<String>) {
    let voiced = c.voice.iter().any(|v| has(out, v)) && !c.generic.iter().any(|g| has(out, g));
    let as_transform = super::transforms::Case {
        name: c.name.clone(),
        transform: "custom".into(),
        language: String::new(),
        instruction: String::new(),
        text: c.text.clone(),
        expect: c.expect.clone(),
    };
    let why = super::transforms::judge(&as_transform, out, out);
    let leaks: Vec<String> = b
        .leaks
        .iter()
        .filter(|l| has(out, l) && !has(&c.text, l) && !has(&c.instruction, l))
        .cloned()
        .collect();
    (voiced, why, leaks)
}

/// Every case `repeat` times in each phase on one model.
pub async fn run(b: &Bench, ep: &Endpoint<'_>, repeat: u32, phases: &[&'static str]) -> Value {
    let mut rows = Vec::new();
    for phase in phases {
        for c in &b.cases {
            for rep in 0..repeat.max(1) {
                let name = if repeat > 1 { format!("{}#{}", c.name, rep + 1) } else { c.name.clone() };
                record::set_context(json!({ "model": ep.model, "scenario": format!("samples:{name}:{phase}") }));
                record::begin_turn();
                let paced0 = record::paced_ms();
                let t0 = Instant::now();
                let (out, error) = ask(b, c, phase, ep).await;
                let ms = (t0.elapsed().as_millis() as u64).saturating_sub(record::paced_ms().saturating_sub(paced0));
                let st = record::end_turn();
                let (voiced, mut why, leaks) = judge(b, c, &out);
                if let Some(e) = &error {
                    why.insert(0, format!("the call failed: {e}"));
                }
                println!(
                    "  {:<7} {:<26} voice {:<3} kept {:<3} leaks {} {:.1} s, prompt {} tok ({} read)",
                    phase,
                    name,
                    if voiced { "yes" } else { "no" },
                    if why.is_empty() { "yes" } else { "no" },
                    leaks.len(),
                    ms as f64 / 1000.0,
                    st.prompt_tokens,
                    st.prompt_eval_tokens
                );
                rows.push(json!({
                    "case": name,
                    "call": c.call,
                    "app": c.app,
                    "phase": phase,
                    "voiced": voiced,
                    "kept": why.is_empty() && error.is_none(),
                    "reasons": why,
                    "leaks": leaks,
                    "ms": ms,
                    "prompt_tokens": st.prompt_tokens,
                    "processed_tokens": st.prompt_eval_tokens,
                    "completion_tokens": st.completion_tokens,
                    "block_chars": block_for(b, c, phase, ep.base_url).map(|x| x.chars().count()).unwrap_or(0),
                    "text": c.text,
                    "instruction": c.instruction,
                    "output": record::redact(&out),
                }));
            }
        }
    }
    let per_phase: Vec<Value> = phases
        .iter()
        .map(|p| {
            let mine: Vec<&Value> = rows.iter().filter(|r| r["phase"] == *p).collect();
            let n = mine.len().max(1) as f64;
            let count = |k: &str| mine.iter().filter(|r| r[k] == true).count();
            let mut ms: Vec<u64> = mine.iter().filter_map(|r| r["ms"].as_u64()).collect();
            ms.sort_unstable();
            let mean = |k: &str| (mine.iter().filter_map(|r| r[k].as_u64()).sum::<u64>() as f64 / n).round() as u64;
            let by_call: serde_json::Map<String, Value> = ["transform", "rewrite", "cleanup"]
                .iter()
                .map(|call| {
                    let of: Vec<&&Value> = mine.iter().filter(|r| r["call"] == *call).collect();
                    (call.to_string(), json!(format!("{}/{}", of.iter().filter(|r| r["voiced"] == true).count(), of.len())))
                })
                .collect();
            json!({
                "phase": p,
                "cases": mine.len(),
                "voiced": count("voiced"),
                "kept": count("kept"),
                "leaks": mine.iter().map(|r| r["leaks"].as_array().map_or(0, Vec::len)).sum::<usize>(),
                "voiced_by_call": by_call,
                "median_ms": ms.get(ms.len() / 2).copied().unwrap_or(0),
                "mean_ms": mean("ms"),
                "mean_prompt_tokens": mean("prompt_tokens"),
                "mean_processed_tokens": mean("processed_tokens"),
                "mean_block_chars": mean("block_chars"),
            })
        })
        .collect();
    json!({ "phases": per_phase, "rows": rows })
}

/// samples.md: a table per model and phase, then each case's result.
pub fn markdown(results: &[(String, Value)]) -> String {
    let mut out = String::from(
        "# Writing samples\n\nEach case without the samples and with them. Voice: one of the person's markers and none of the generic ones. Kept: the facts kept, nothing answered. Leaks: a sample's own facts in the result.\n\n| Model | Phase | Voice | By call (voice) | Kept | Leaks | Median ms | Mean ms | Prompt tok | Read tok | Block chars |\n|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for (label, r) in results {
        for p in r["phases"].as_array().into_iter().flatten() {
            let by: Vec<String> = p["voiced_by_call"].as_object().into_iter().flatten().map(|(k, v)| format!("{k} {}", v.as_str().unwrap_or(""))).collect();
            out.push_str(&format!(
                "| {label} | {} | {}/{} | {} | {}/{} | {} | {} | {} | {} | {} | {} |\n",
                p["phase"].as_str().unwrap_or(""),
                p["voiced"],
                p["cases"],
                by.join(", "),
                p["kept"],
                p["cases"],
                p["leaks"],
                p["median_ms"],
                p["mean_ms"],
                p["mean_prompt_tokens"],
                p["mean_processed_tokens"],
                p["mean_block_chars"],
            ));
        }
    }
    for (label, r) in results {
        out.push_str(&format!("\n## {label}\n"));
        for row in r["rows"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "\n### {} ({}, {}, {}) — voice {}, kept {}\n",
                row["case"].as_str().unwrap_or(""),
                row["call"].as_str().unwrap_or(""),
                row["app"].as_str().unwrap_or(""),
                row["phase"].as_str().unwrap_or(""),
                row["voiced"],
                row["kept"],
            ));
            for why in row["reasons"].as_array().into_iter().flatten() {
                out.push_str(&format!("- {}\n", why.as_str().unwrap_or("")));
            }
            for l in row["leaks"].as_array().into_iter().flatten() {
                out.push_str(&format!("- took {:?} from a sample\n", l.as_str().unwrap_or("")));
            }
            let input = if row["call"] == "rewrite" { row["instruction"].as_str().unwrap_or("") } else { row["text"].as_str().unwrap_or("") };
            out.push_str(&format!("\nIn:\n\n```\n{input}\n```\n\nOut:\n\n```\n{}\n```\n", row["output"].as_str().unwrap_or("")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bench() -> Bench {
        load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/samples/cases.json"), &[]).unwrap()
    }

    #[test]
    fn the_cases_file_reads_with_every_call_and_kind() {
        let b = bench();
        assert!(b.samples.len() >= 4 && !b.leaks.is_empty());
        for call in ["transform", "rewrite", "cleanup"] {
            assert!(b.cases.iter().filter(|c| c.call == call).count() >= 2, "{call}");
        }
        for c in &b.cases {
            assert!(Category::from_id(&c.app).is_some(), "{}: app", c.name);
            assert!(!c.voice.is_empty(), "{}: no voice markers", c.name);
            if c.call == "transform" {
                assert!(crate::dictation::transforms::builtin_instruction(&c.transform, "").is_some(), "{}", c.name);
            }
            if c.call == "rewrite" {
                assert!(!c.instruction.is_empty(), "{}", c.name);
            }
            // With: a block for the case's kind, never bigger than the cloud's budget.
            let block = block_for(&b, c, "with", "https://api.example.com/v1").unwrap();
            assert!(block.chars().count() < writing_samples::BUDGET_CLOUD + 600);
            assert!(block_for(&b, c, "without", "https://api.example.com/v1").is_none());
        }
    }

    #[test]
    fn the_judge_hears_the_voice_and_catches_a_leak() {
        let b = bench();
        let c = b.cases.iter().find(|c| c.app == "email").unwrap();
        let (voiced, _, leaks) = judge(&b, c, "Hiya all,\n\nThe fire alarm test is on Friday.\n\nCheers,\nRobin");
        assert!(voiced && leaks.is_empty());
        let (voiced, _, _) = judge(&b, c, "Dear team,\n\nThe test is on Friday!\n\nBest regards,\nRobin");
        assert!(!voiced);
        let (_, _, leaks) = judge(&b, c, "Hiya all, see you at Harbour Street. Cheers");
        assert_eq!(leaks, vec!["Harbour Street".to_string()]);
    }
}

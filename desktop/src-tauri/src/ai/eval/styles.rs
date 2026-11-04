//! `--styles`: **styles by app** (`cleanup_styles.rs`) on each `--model` —
//! does an email come out formal, a chat stay casual, an AI prompt stay
//! literal (no "please", the meaning intact), a terminal command stay
//! untouched, a filler-only dictation stay empty — and what does it cost?
//!
//! Each case in `evals/cleanup/styles.json` is cleaned through the
//! dictation's own code — `cleanup_styles::resolve` for a stand-in app of
//! its kind (`sample_target`), then `cleanup_styles::clean` — twice:
//!
//! - **before**: the call every dictation made until 24 Oct 2026
//!   (`llm::cleanup_with_names` with the main body, whatever the app);
//! - **after**: with its kind's style and the case's tone (and filler
//!   alone never sent, `cleanup_styles::filler_only`).
//!
//! Each output is checked (the case's `checks`, [`failures`]); per phase,
//! the pass rate overall and per kind, and the wall time per kind (mean,
//! median) with llama.cpp's prompt tokens processed — the cases mix the
//! kinds, as dictations into different apps would, so the time includes
//! moving from one kind's line to another's. `--styles-phase before|after`
//! runs one phase (a cloud model's tokens), `--styles-quick` the cases
//! marked `quick`, `--cleanup-rounds` the rounds.

use std::path::Path;
use std::time::Instant;

use serde_json::{json, Value};

use super::cleanup::Endpoint;
use super::record;
use crate::ai::cleanup_styles::{self, Category, Tone};

#[derive(Debug, serde::Deserialize)]
pub struct Bench {
    pub cases: Vec<Case>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Case {
    pub name: String,
    pub category: String,
    pub tone: String,
    pub text: String,
    #[serde(default)]
    pub quick: bool,
    #[serde(default)]
    pub checks: Checks,
}

/// What a cleaned text must (not) be. See styles.json's description.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct Checks {
    pub has: Vec<String>,
    pub has_any: Vec<Vec<String>>,
    pub not: Vec<String>,
    pub exact: Option<String>,
    pub starts_with: Option<String>,
    pub lines_min: Option<usize>,
    pub lowercase: bool,
    pub keep: Vec<String>,
    pub no_final_period: bool,
    pub empty: bool,
    pub max_words: Option<usize>,
}

pub const PHASES: &[&str] = &["before", "after"];

pub fn load(path: &Path) -> Result<Bench, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let b: Bench = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    for c in &b.cases {
        let cat = Category::from_id(&c.category).ok_or_else(|| format!("{}: no kind of app {}", c.name, c.category))?;
        let tone = Tone::from_id(&c.tone).ok_or_else(|| format!("{}: no tone {}", c.name, c.tone))?;
        if !cat.tones().contains(&tone) {
            return Err(format!("{}: {} doesn't offer {}", c.name, c.category, c.tone));
        }
    }
    if b.cases.is_empty() {
        return Err(format!("{}: no cases", path.display()));
    }
    Ok(b)
}

/// `needle` in `hay` as whole words, any case (a needle that starts or ends
/// with a symbol, like "```", as it is). Pure.
fn has_words(hay: &str, needle: &str) -> bool {
    let (h, n) = (hay.to_lowercase(), needle.to_lowercase());
    if n.is_empty() {
        return false;
    }
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut from = 0;
    while let Some(i) = h[from..].find(&n) {
        let at = from + i;
        let before = h[..at].chars().next_back();
        let after = h[at + n.len()..].chars().next();
        let lead_ok = !word(n.chars().next()) || !word(before);
        let tail_ok = !word(n.chars().next_back()) || !word(after);
        if lead_ok && tail_ok {
            return true;
        }
        from = at + n.len().max(1);
    }
    false
}

/// The checks `output` fails (empty: it passes). Pure.
pub fn failures(checks: &Checks, output: &str) -> Vec<String> {
    let out = output.trim();
    let lower = out.to_lowercase();
    let mut f = Vec::new();
    if checks.empty {
        if !out.is_empty() {
            f.push("not empty".to_string());
        }
        return f;
    }
    if out.is_empty() {
        f.push("empty".to_string());
        return f;
    }
    if let Some(x) = &checks.exact {
        if out != x {
            f.push(format!("not exactly \"{x}\""));
        }
    }
    for h in &checks.has {
        if !lower.contains(&h.to_lowercase()) {
            f.push(format!("no \"{h}\""));
        }
    }
    for any in &checks.has_any {
        if !any.iter().any(|h| lower.contains(&h.to_lowercase())) {
            f.push(format!("none of {any:?}"));
        }
    }
    for n in &checks.not {
        if has_words(out, n) {
            f.push(format!("has \"{n}\""));
        }
    }
    if let Some(s) = &checks.starts_with {
        if !out.starts_with(s.as_str()) {
            f.push(format!("doesn't start \"{s}\""));
        }
    }
    if let Some(n) = checks.lines_min {
        let lines = out.lines().filter(|l| !l.trim().is_empty()).count();
        if lines < n {
            f.push(format!("{lines} line(s), not {n}+"));
        }
    }
    if checks.lowercase {
        let capital = out
            .split_whitespace()
            .filter(|w| !checks.keep.iter().any(|k| w.trim_matches(|c: char| !c.is_alphanumeric()) == k))
            .find(|w| w.chars().any(char::is_uppercase));
        if let Some(w) = capital {
            f.push(format!("a capital: \"{w}\""));
        }
    }
    if checks.no_final_period && out.ends_with('.') && !out.ends_with("...") {
        f.push("ends with a full stop".to_string());
    }
    if let Some(max) = checks.max_words {
        let n = out.split_whitespace().count();
        if n > max {
            f.push(format!("{n} words, over {max}"));
        }
    }
    f
}

/// One cleaned case.
#[derive(Debug, Clone, serde::Serialize)]
struct Shot {
    phase: &'static str,
    case: String,
    category: String,
    tone: String,
    input: String,
    output: String,
    error: Option<String>,
    failures: Vec<String>,
    ms: u64,
    /// A model was asked (not filler alone, not "As said").
    asked: bool,
    processed_tokens: u64,
    gen_tokens: u64,
}

/// The settings a phase runs on: the defaults, every kind's style there,
/// "Match the style to the app" on (after) or off (before), and the case's
/// tone for its kind.
fn settings(case: &Case, after: bool) -> crate::config::YapConfig {
    let mut cfg = crate::config::YapConfig::default();
    cleanup_styles::on_load(&mut cfg);
    cfg.cleanup_styles = Some(after);
    if let Some(p) = cfg.cleanup_profiles.iter_mut().find(|p| p.category == case.category) {
        p.tone = case.tone.clone();
    }
    cfg
}

async fn clean(phase: &'static str, case: &Case, ep: &Endpoint<'_>) -> Shot {
    let cfg = settings(case, phase == "after");
    let category = Category::from_id(&case.category).unwrap_or(Category::Other);
    let spelling = crate::ai::about_you::cleanup_spelling(&cfg);
    record::begin_turn();
    // Time held back to keep under a cloud plan's tokens a minute (Groq's
    // 8,000) isn't the cleanup's.
    let paced0 = record::paced_ms();
    let t0 = Instant::now();
    let mut asked = true;
    let out = if phase == "before" {
        // The call every dictation made until styles: the main body, the
        // model asked whatever the text.
        crate::ai::llm::cleanup_with_names(&case.text, ep.base_url, ep.key, ep.model, ep.provider, &cfg.pp_prompt, spelling.as_deref(), None, &[], false)
            .await
    } else {
        match cleanup_styles::resolve(&cfg, &cleanup_styles::sample_target(category)) {
            Some(plan) => {
                asked = !cleanup_styles::filler_only(&case.text);
                cleanup_styles::clean(&case.text, ep.base_url, ep.key, ep.model, ep.provider, &plan, spelling.as_deref(), None, &[], false)
                    .await
            }
            // As said: the transcript, no model.
            None => {
                asked = false;
                Ok(case.text.clone())
            }
        }
    };
    let ms = (t0.elapsed().as_millis() as u64).saturating_sub(record::paced_ms().saturating_sub(paced0));
    let st = record::end_turn();
    let (output, error) = match out {
        Ok(o) => (o, None),
        Err(e) => (String::new(), Some(e)),
    };
    let failures = if error.is_some() { vec!["error".to_string()] } else { failures(&case.checks, &output) };
    Shot {
        phase,
        case: case.name.clone(),
        category: case.category.clone(),
        tone: case.tone.clone(),
        input: case.text.clone(),
        output,
        error,
        failures,
        ms,
        asked,
        processed_tokens: st.prompt_eval_tokens,
        gen_tokens: st.gen_tokens,
    }
}

/// Run the cases on one model: `phases` (before, after or both) × `rounds`.
/// `only`: case names containing one of these (`--only`; empty = all).
pub async fn run(bench: &Bench, ep: &Endpoint<'_>, rounds: u32, phases: &[&'static str], quick: bool, only: &[String]) -> Value {
    let cases: Vec<&Case> = bench
        .cases
        .iter()
        .filter(|c| !quick || c.quick)
        .filter(|c| only.is_empty() || only.iter().any(|o| c.name.contains(o.as_str())))
        .collect();
    let mut shots: Vec<Shot> = Vec::new();
    // Each case before and after in turn: the two phases meet the same
    // server, whatever else the PC is doing at that moment.
    for round in 0..rounds.max(1) {
        for case in &cases {
            for &phase in phases {
                let s = clean(phase, case, ep).await;
                if round == 0 {
                    println!(
                        "  {phase:<6} {:<30} {:>5} ms  {}",
                        case.name,
                        s.ms,
                        if s.failures.is_empty() { "pass".to_string() } else { format!("FAIL: {}", s.failures.join("; ")) }
                    );
                }
                shots.push(s);
            }
        }
    }
    let mut summary: Vec<Value> = Vec::new();
    for &phase in phases {
        let mine: Vec<&Shot> = shots.iter().filter(|s| s.phase == phase).collect();
        let passed = mine.iter().filter(|s| s.failures.is_empty()).count();
        // Times: only the cleanups a model was asked for.
        let mut kinds: Vec<Value> = Vec::new();
        for c in Category::ALL {
            let of: Vec<&&Shot> = mine.iter().filter(|s| s.category == c.id()).collect();
            if of.is_empty() {
                continue;
            }
            let timed: Vec<&&&Shot> = of.iter().filter(|s| s.asked).collect();
            let mut ms: Vec<u64> = timed.iter().map(|s| s.ms).collect();
            ms.sort_unstable();
            let n = timed.len().max(1) as u64;
            kinds.push(json!({
                "category": c.id(),
                "n": of.len(),
                "passed": of.iter().filter(|s| s.failures.is_empty()).count(),
                "mean_ms": ms.iter().sum::<u64>() / n,
                "median_ms": ms.get(ms.len() / 2).copied().unwrap_or(0),
                "mean_processed_tokens": timed.iter().map(|s| s.processed_tokens).sum::<u64>() / n,
                "mean_gen_tokens": timed.iter().map(|s| s.gen_tokens).sum::<u64>() / n,
            }));
        }
        let mut ms: Vec<u64> = mine.iter().filter(|s| s.asked).map(|s| s.ms).collect();
        ms.sort_unstable();
        let mean = if ms.is_empty() { 0 } else { ms.iter().sum::<u64>() / ms.len() as u64 };
        println!(
            "  {phase}: {passed}/{} pass ({:.0}%), mean {mean} ms",
            mine.len(),
            if mine.is_empty() { 0.0 } else { passed as f64 * 100.0 / mine.len() as f64 }
        );
        summary.push(json!({
            "phase": phase,
            "n": mine.len(),
            "passed": passed,
            "mean_ms": mean,
            "median_ms": ms.get(ms.len() / 2).copied().unwrap_or(0),
            "kinds": kinds,
        }));
    }
    json!({ "phases": summary, "shots": shots })
}

/// styles.md: pass rates and times per model and phase, then every output.
pub fn markdown(models: &[(String, Value)]) -> String {
    let mut md = String::from(
        "# Styles by app\n\nEach case cleaned as before styles (the main cleanup alone) and with its kind's style; pass = every check held. Times are the cleanup's wall time (ms), with llama.cpp's prompt tokens processed, over the cleanups a model was asked for (after styles, filler alone and \"As said\" ask none).\n\n| Model | Phase | Passed | Mean ms | Median ms |\n|---|---|---|---|---|\n",
    );
    for (label, r) in models {
        for p in r["phases"].as_array().into_iter().flatten() {
            md.push_str(&format!(
                "| {label} | {} | {}/{} | {} | {} |\n",
                p["phase"].as_str().unwrap_or(""),
                p["passed"],
                p["n"],
                p["mean_ms"],
                p["median_ms"]
            ));
        }
    }
    md.push_str("\n## Per kind of app\n\n| Model | Phase | Kind | Passed | Mean ms | Median ms | Prompt tok processed | Gen tok |\n|---|---|---|---|---|---|---|---|\n");
    for (label, r) in models {
        for p in r["phases"].as_array().into_iter().flatten() {
            for k in p["kinds"].as_array().into_iter().flatten() {
                md.push_str(&format!(
                    "| {label} | {} | {} | {}/{} | {} | {} | {} | {} |\n",
                    p["phase"].as_str().unwrap_or(""),
                    k["category"].as_str().unwrap_or(""),
                    k["passed"],
                    k["n"],
                    k["mean_ms"],
                    k["median_ms"],
                    k["mean_processed_tokens"],
                    k["mean_gen_tokens"]
                ));
            }
        }
    }
    md.push_str("\n## Outputs\n");
    for (label, r) in models {
        md.push_str(&format!("\n### {label}\n\n"));
        for s in r["shots"].as_array().into_iter().flatten() {
            let out = match s["error"].as_str() {
                Some(e) => format!("(error: {e})"),
                None => s["output"].as_str().unwrap_or("").replace('\n', " / "),
            };
            let verdict = match s["failures"].as_array().filter(|f| !f.is_empty()) {
                Some(f) => format!("FAIL: {}", f.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; ")),
                None => "pass".to_string(),
            };
            md.push_str(&format!(
                "- {} · {} ({}/{}): \"{}\" → \"{}\" — {verdict} ({} ms)\n",
                s["phase"].as_str().unwrap_or(""),
                s["case"].as_str().unwrap_or(""),
                s["category"].as_str().unwrap_or(""),
                s["tone"].as_str().unwrap_or(""),
                s["input"].as_str().unwrap_or(""),
                out,
                s["ms"]
            ));
        }
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cases_parse_and_every_kind_has_some() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/cleanup/styles.json");
        let b = load(&path).expect("styles.json");
        for c in Category::ALL {
            assert!(b.cases.iter().any(|x| x.category == c.id()), "{c:?}");
        }
        // A cloud model's run keeps to ~8 cleanups (Groq's free plan).
        assert!(b.cases.iter().filter(|c| c.quick).count() <= 8);
    }

    #[test]
    fn the_checks() {
        let c = |json: Value| serde_json::from_value::<Checks>(json).unwrap();
        assert!(failures(&c(json!({ "exact": "git status" })), "git status").is_empty());
        assert_eq!(failures(&c(json!({ "exact": "git status" })), "Git status."), vec!["not exactly \"git status\""]);
        assert!(failures(&c(json!({ "empty": true })), "  ").is_empty());
        assert_eq!(failures(&c(json!({ "empty": true })), "Um."), vec!["not empty"]);
        // Whole words: "this" has no "hi"; "Hi," has.
        let no_hi = c(json!({ "not": ["Hi", "```"] }));
        assert!(failures(&no_hi, "this is it").is_empty());
        assert_eq!(failures(&no_hi, "Hi, it's me"), vec!["has \"Hi\""]);
        assert_eq!(failures(&no_hi, "```py"), vec!["has \"```\""]);
        let casual = c(json!({ "lowercase": true, "no_final_period": true, "keep": ["Sam"], "max_words": 5 }));
        assert!(failures(&casual, "see you at 6, Sam?").is_empty());
        assert_eq!(failures(&casual, "See you at six with you all."), vec![
            "a capital: \"See\"".to_string(),
            "ends with a full stop".to_string(),
            "7 words, over 5".to_string()
        ]);
        let email = c(json!({ "starts_with": "Hi Sarah", "lines_min": 2, "has_any": [["I will"]] }));
        assert!(failures(&email, "Hi Sarah,\n\nI will send it.").is_empty());
        assert_eq!(failures(&email, "Hi Sarah, I'll send it.").len(), 2);
    }
}

//! `--languages`: **My languages** (`dictation/languages.rs`) on each
//! `--model`, through the dictation's own calls, over
//! `evals/languages/cases.json`:
//!
//! - **cleanup** cases — a dictation in French, Spanish or German as
//!   speech-to-text writes it (fillers, no punctuation), for a kind of app
//!   (`cleanup_styles::resolve` for its stand-in, `sample_target`), cleaned
//!   by `cleanup_styles::clean` twice: **before** (the call every dictation
//!   made until 25 Oct 2026: no word about the language) and **after** (with
//!   `languages::language_line` — the language Whisper heard, or the ones
//!   it may be in — just before the transcript, and no English spelling
//!   line for another language). Must stay in its language and keep its
//!   names;
//! - **translate** cases — a language shortcut that pastes another
//!   language: `llm::transform` with `languages::translate_instruction` and
//!   `transforms::clean_output`, as `languages::translate_dictation` sends
//!   it. Must come out in the target language, names kept, a question
//!   translated rather than answered, the fillers gone.
//!
//! Each result is judged as a transform's is (`eval::transforms::judge`:
//! `keeps`, `not_contains`, `contains_any`, the length ratios).
//! `--languages-phase before|after` runs one cleanup phase (a cloud model's
//! tokens: a cleanup call is ~1.7k). `languages.md` in the run folder; the
//! numbers in `summary.json` (`models[].languages`).

use std::path::Path;
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};

use super::cleanup::Endpoint;
use super::record;
use crate::ai::cleanup_styles::{self, Category};

/// The cleanup phases.
pub const PHASES: &[&str] = &["before", "after"];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    /// "cleanup" | "translate".
    pub kind: String,
    /// Cleanup: the language Yap knows the dictation is in (Whisper heard
    /// it, or a shortcut's); "" — not known.
    #[serde(default)]
    pub language: String,
    /// Cleanup: My languages.
    #[serde(default)]
    pub mine: Vec<String>,
    /// Cleanup: the kind of app (`cleanup_styles` category id; "other").
    #[serde(default)]
    pub app: String,
    /// Translate: the language spoken ("" — not known) and the one pasted.
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
    pub text: String,
    #[serde(default)]
    pub expect: super::transforms::Expect,
}

pub fn load(path: &Path, only: &[String]) -> Result<Vec<Case>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    let mut cases: Vec<Case> = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let only: Vec<String> = only.iter().map(|s| s.to_lowercase()).collect();
    cases.retain(|c| only.is_empty() || only.iter().any(|o| c.name.to_lowercase().contains(o) || c.kind == *o));
    if cases.is_empty() {
        return Err(format!("no language cases in {}", path.display()));
    }
    Ok(cases)
}

/// The language line a cleanup case's dictation gets in a phase: the app's
/// (`languages::language_line`), or — to compare wordings without a
/// rebuild — `YAP_EVAL_LANGUAGE_LINE` for a known language ("{n}" its
/// name, "{f}" its fillers) and `YAP_EVAL_LANGUAGE_MAYBE` for one that isn't
/// ("{names}" the languages it may be in, "{f}" their fillers).
pub fn line_for(c: &Case, phase: &str) -> Option<String> {
    if phase != "after" {
        return None;
    }
    use crate::dictation::languages::catalog;
    let known = (!c.language.is_empty()).then_some(c.language.as_str());
    let fillers = |codes: &[&str]| codes.iter().filter_map(|c| catalog::fillers(c)).collect::<Vec<_>>().join(", ");
    match (known, std::env::var("YAP_EVAL_LANGUAGE_LINE"), std::env::var("YAP_EVAL_LANGUAGE_MAYBE")) {
        (Some(code), Ok(t), _) if !t.is_empty() => Some(t.replace("{n}", &catalog::name(code)).replace("{f}", &fillers(&[code]))),
        (None, _, Ok(t)) if !t.is_empty() => {
            let codes: Vec<&str> = c.mine.iter().map(String::as_str).collect();
            Some(t.replace("{names}", &catalog::names_joined(&c.mine, "or")).replace("{f}", &fillers(&codes)))
        }
        _ => crate::dictation::languages::language_line(known, &c.mine),
    }
}

/// One case, once: the text as the app would paste it, the model's own,
/// and an error.
async fn ask(c: &Case, phase: &str, ep: &Endpoint<'_>) -> (String, String, Option<String>) {
    if c.kind == "translate" {
        let from = (!c.from.is_empty()).then_some(c.from.as_str());
        let instruction = crate::dictation::languages::translate_instruction(from, &c.to);
        return match crate::ai::llm::transform(&instruction, &c.text, ep.base_url, ep.key, ep.model, ep.provider, false).await {
            Ok(raw) => (crate::dictation::transforms::clean_output(&raw), raw, None),
            Err(e) => (String::new(), String::new(), Some(record::redact(&e))),
        };
    }
    let mut cfg = crate::config::YapConfig::default();
    cleanup_styles::on_load(&mut cfg);
    let category = Category::from_id(if c.app.is_empty() { "other" } else { &c.app }).unwrap_or(Category::Other);
    // English spelling (the run's --locale) only where English may be it.
    let english = c.language.is_empty() || c.language == "en" || phase == "before";
    let mut spelling = if english { crate::ai::about_you::cleanup_spelling(&cfg) } else { None };
    let mut line = line_for(c, phase);
    // Where the line goes, to compare (`YAP_EVAL_LANGUAGE_AT`): just before
    // the transcript (the app's, default), or where the spelling line goes
    // (the system prompt and the restated instruction: "instruction"), or
    // both.
    match std::env::var("YAP_EVAL_LANGUAGE_AT").as_deref() {
        Ok("instruction") => spelling = line.take().or(spelling),
        Ok("both") => spelling = line.clone().or(spelling),
        _ => {}
    }
    match cleanup_styles::resolve(&cfg, &cleanup_styles::sample_target(category)) {
        Some(plan) => match cleanup_styles::clean(&c.text, ep.base_url, ep.key, ep.model, ep.provider, &plan, spelling.as_deref(), line.as_deref(), &[], false).await {
            // After: a filler left at the start comes off, as the app's
            // `languages::tidy_cleaned` does (the language, else My languages).
            Ok(out) if phase == "after" => {
                let codes: Vec<&str> = if c.language.is_empty() { c.mine.iter().map(String::as_str).collect() } else { vec![c.language.as_str()] };
                (crate::dictation::languages::drop_leading_filler(&out, &codes), out, None)
            }
            Ok(out) => (out.clone(), out, None),
            Err(e) => (String::new(), String::new(), Some(record::redact(&e))),
        },
        None => (c.text.clone(), c.text.clone(), None),
    }
}

/// Why `out` fails `c` (`transforms::judge`'s rules).
pub fn judge(c: &Case, raw: &str, out: &str) -> Vec<String> {
    let as_transform = super::transforms::Case {
        name: c.name.clone(),
        transform: "custom".into(),
        language: String::new(),
        instruction: String::new(),
        text: c.text.clone(),
        expect: c.expect.clone(),
    };
    super::transforms::judge(&as_transform, raw, out)
}

/// Every case `repeat` times on one model (cleanup cases in each of
/// `phases`); its results for summary.json and languages.md.
pub async fn run(cases: &[Case], ep: &Endpoint<'_>, repeat: u32, phases: &[&'static str]) -> Value {
    let mut rows = Vec::new();
    let mut tally: std::collections::BTreeMap<String, (usize, usize)> = std::collections::BTreeMap::new();
    let mut ms_all: Vec<u64> = Vec::new();
    let mut tokens = 0u64;
    for c in cases {
        let its_phases: Vec<&str> = if c.kind == "translate" { vec!["translate"] } else { phases.to_vec() };
        for phase in its_phases {
            for rep in 0..repeat.max(1) {
                let name = if repeat > 1 { format!("{}#{}", c.name, rep + 1) } else { c.name.clone() };
                record::set_context(json!({ "model": ep.model, "scenario": format!("languages:{name}:{phase}") }));
                record::begin_turn();
                let paced0 = record::paced_ms();
                let t0 = Instant::now();
                let (out, raw, error) = ask(c, phase, ep).await;
                let ms = (t0.elapsed().as_millis() as u64).saturating_sub(record::paced_ms().saturating_sub(paced0));
                let stats = record::end_turn();
                tokens += stats.total_tokens;
                let mut why = judge(c, &raw, &out);
                if let Some(e) = &error {
                    why.insert(0, format!("the call failed: {e}"));
                }
                let pass = why.is_empty();
                let t = tally.entry(phase.to_string()).or_default();
                t.1 += 1;
                t.0 += usize::from(pass);
                if error.is_none() {
                    ms_all.push(ms);
                }
                println!(
                    "  {:<4} {:<9} {:<34} {:.1} s{}",
                    if pass { "PASS" } else { "FAIL" },
                    phase,
                    name,
                    ms as f64 / 1000.0,
                    why.first().map(|w| format!(" | {w}")).unwrap_or_default()
                );
                rows.push(json!({
                    "case": name,
                    "kind": c.kind,
                    "phase": phase,
                    "status": if pass { "pass" } else { "fail" },
                    "reasons": why,
                    "ms": ms,
                    "prompt_tokens": stats.prompt_tokens,
                    "completion_tokens": stats.completion_tokens,
                    "line": line_for(c, phase),
                    "text": c.text,
                    "output": record::redact(&out),
                }));
            }
        }
    }
    ms_all.sort_unstable();
    json!({
        "cases": rows.len(),
        "passed": rows.iter().filter(|r| r["status"] == "pass").count(),
        "tokens": tokens,
        "median_ms": ms_all.get(ms_all.len() / 2).copied().unwrap_or(0),
        "by_phase": tally.iter().map(|(k, (p, n))| (k.clone(), json!(format!("{p}/{n}")))).collect::<serde_json::Map<_, _>>(),
        "rows": rows,
    })
}

/// languages.md: a table per model, then each case's text and result.
pub fn markdown(results: &[(String, Value)]) -> String {
    let mut out = String::from("# My languages\n\n| Model | Passed | Per phase | Median | Tokens |\n|---|---|---|---|---|\n");
    for (label, r) in results {
        let per: Vec<String> = r["by_phase"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| format!("{k} {}", v.as_str().unwrap_or("")))
            .collect();
        out.push_str(&format!(
            "| {label} | {}/{} | {} | {:.1} s | {} |\n",
            r["passed"],
            r["cases"],
            per.join(", "),
            r["median_ms"].as_f64().unwrap_or(0.0) / 1000.0,
            r["tokens"],
        ));
    }
    for (label, r) in results {
        out.push_str(&format!("\n## {label}\n"));
        for row in r["rows"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "\n### {} ({}, {}) — {}\n",
                row["case"].as_str().unwrap_or(""),
                row["kind"].as_str().unwrap_or(""),
                row["phase"].as_str().unwrap_or(""),
                row["status"].as_str().unwrap_or("").to_uppercase()
            ));
            for why in row["reasons"].as_array().into_iter().flatten() {
                out.push_str(&format!("- {}\n", why.as_str().unwrap_or("")));
            }
            if let Some(line) = row["line"].as_str() {
                out.push_str(&format!("\nLine: {line}\n"));
            }
            out.push_str(&format!(
                "\nText:\n\n```\n{}\n```\n\nResult:\n\n```\n{}\n```\n",
                row["text"].as_str().unwrap_or(""),
                row["output"].as_str().unwrap_or("")
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cases_file_reads_with_cleanup_in_three_languages_and_translations() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/languages/cases.json");
        let cases = load(&path, &[]).unwrap();
        for lang in ["fr", "es", "de"] {
            assert!(cases.iter().filter(|c| c.kind == "cleanup" && (c.language == lang || c.mine.iter().any(|m| m == lang))).count() >= 2, "{lang}");
        }
        assert!(cases.iter().filter(|c| c.kind == "translate").count() >= 3);
        for c in &cases {
            assert!(["cleanup", "translate"].contains(&c.kind.as_str()), "{}", c.name);
            if c.kind == "translate" {
                assert!(crate::dictation::languages::catalog::find(&c.to).is_some(), "{}", c.name);
            } else {
                assert!(c.app.is_empty() || Category::from_id(&c.app).is_some(), "{}", c.name);
                assert!(line_for(c, "after").is_some(), "{}: no language line", c.name);
                assert!(line_for(c, "before").is_none());
            }
            assert!(!c.expect.contains_any.is_empty() || !c.expect.keeps.is_empty(), "{}: checks nothing", c.name);
        }
    }
}

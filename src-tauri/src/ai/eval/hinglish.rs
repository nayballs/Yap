//! `--hinglish`: **Hinglish** (`dictation/languages/hinglish.rs`) on each
//! `--model`, over `evals/hinglish/cases.json`: made-up Hindi dictations as
//! Whisper writes them (Devanagari, English words in Devanagari or in Latin
//! letters), with the Hinglish a person would type. Three phases:
//!
//! - **rules**: Yap's own spelling-out alone (`hinglish::to_latin`), what a
//!   dictation gets with AI cleanup off — no model asked;
//! - **devanagari**: AI cleanup given Whisper's Devanagari and the Hinglish
//!   language line (`languages::language_line_with`), anything it still
//!   writes in Devanagari spelt out after;
//! - **latin**: AI cleanup given the rules' Latin letters and the same
//!   line — what a dictation does (`pipeline::deliver`).
//!
//! Judged on: no Devanagari left, the English words kept English, enough of
//! the Hindi words there (not translated), none of the words a translation
//! into English would bring, and how many of the reference's words it has
//! (`overlap`). `hinglish.md` in the run folder; `models[].hinglish` in
//! summary.json.

use std::path::Path;
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};

use super::cleanup::Endpoint;
use super::record;
use crate::ai::cleanup_styles::{self, Category};
use crate::dictation::languages::hinglish;

pub const PHASES: &[&str] = &["rules", "devanagari", "latin"];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    /// The kind of app (`cleanup_styles` category id).
    #[serde(default)]
    pub app: String,
    /// As Whisper writes it.
    pub text: String,
    /// As a person would type it.
    pub reference: String,
    /// Words that must stay English.
    #[serde(default)]
    pub english: Vec<String>,
    /// Hindi words in Latin letters, at least half of which must be there.
    #[serde(default)]
    pub hindi: Vec<String>,
    /// Words an English translation would bring.
    #[serde(default)]
    pub translated: Vec<String>,
}

pub fn load(path: &Path, only: &[String]) -> Result<Vec<Case>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    let mut cases: Vec<Case> = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let only: Vec<String> = only.iter().map(|s| s.to_lowercase()).collect();
    cases.retain(|c| only.is_empty() || only.iter().any(|o| c.name.to_lowercase().contains(o)));
    if cases.is_empty() {
        return Err(format!("no Hinglish cases in {}", path.display()));
    }
    Ok(cases)
}

/// The words of `s`, lower case, letters and digits only.
fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// The share of `reference`'s words `out` has (each counted once per time
/// it's there). Pure.
pub fn overlap(reference: &str, out: &str) -> f64 {
    let want = words(reference);
    if want.is_empty() {
        return 1.0;
    }
    let mut have = words(out);
    let mut found = 0usize;
    for w in &want {
        if let Some(i) = have.iter().position(|h| h == w) {
            have.swap_remove(i);
            found += 1;
        }
    }
    found as f64 / want.len() as f64
}

/// Why `out` fails `c` (pure).
pub fn judge(c: &Case, out: &str) -> Vec<String> {
    let mut why = Vec::new();
    if out.trim().is_empty() {
        return vec!["empty".into()];
    }
    if hinglish::has_devanagari(out) {
        why.push("Devanagari left".into());
    }
    let w = words(out);
    for e in &c.english {
        if !w.contains(&e.to_lowercase()) {
            why.push(format!("lost the English {e:?}"));
        }
    }
    let hindi = c.hindi.iter().filter(|h| w.contains(&h.to_lowercase())).count();
    if !c.hindi.is_empty() && hindi * 2 < c.hindi.len() {
        why.push(format!("{hindi} of {} Hindi words: translated?", c.hindi.len()));
    }
    for t in &c.translated {
        if w.contains(&t.to_lowercase()) {
            why.push(format!("says {t:?}: translated"));
        }
    }
    why
}

/// One case in one phase: the final text, whether the model itself wrote
/// Devanagari, and an error.
async fn ask(c: &Case, phase: &str, ep: &Endpoint<'_>) -> (String, bool, Option<String>) {
    if phase == "rules" {
        return (hinglish::to_latin(&c.text), false, None);
    }
    let input = if phase == "latin" { hinglish::to_latin(&c.text) } else { c.text.clone() };
    let mut cfg = crate::config::YapConfig::default();
    cleanup_styles::on_load(&mut cfg);
    let mine = vec!["en".to_string(), "hi".to_string()];
    let line = crate::dictation::languages::language_line_with(Some("hi"), &mine, true);
    let category = Category::from_id(if c.app.is_empty() { "other" } else { &c.app }).unwrap_or(Category::Other);
    match cleanup_styles::resolve(&cfg, &cleanup_styles::sample_target(category)) {
        Some(plan) => match cleanup_styles::clean(&input, ep.base_url, ep.key, ep.model, ep.provider, &plan, None, line.as_deref(), &[], false).await {
            Ok(out) => {
                let wrote = hinglish::has_devanagari(&out);
                // Latin: as a dictation does, a cleanup that translated it
                // gives way to the rules' text (`languages::hinglish_kept`).
                if phase == "latin" && !hinglish::kept_the_words(&input, &out) {
                    println!("    (cleanup translated it: the rules' text instead)");
                    return (input, wrote, None);
                }
                (hinglish::to_latin(&out), wrote, None)
            }
            Err(e) => (String::new(), false, Some(record::redact(&e))),
        },
        None => (input, false, None),
    }
}

/// Every case `repeat` times in each phase on one model.
pub async fn run(cases: &[Case], ep: &Endpoint<'_>, repeat: u32, phases: &[&'static str]) -> Value {
    let mut rows = Vec::new();
    for phase in phases {
        for c in cases {
            let reps = if *phase == "rules" { 1 } else { repeat.max(1) };
            for rep in 0..reps {
                let name = if reps > 1 { format!("{}#{}", c.name, rep + 1) } else { c.name.clone() };
                record::set_context(json!({ "model": ep.model, "scenario": format!("hinglish:{name}:{phase}") }));
                record::begin_turn();
                let t0 = Instant::now();
                let (out, wrote_devanagari, error) = ask(c, phase, ep).await;
                let ms = t0.elapsed().as_millis() as u64;
                let st = record::end_turn();
                let mut why = judge(c, &out);
                if let Some(e) = &error {
                    why.insert(0, format!("the call failed: {e}"));
                }
                let score = overlap(&c.reference, &out);
                println!(
                    "  {:<4} {:<10} {:<28} overlap {:.2}{} {:.1} s{}",
                    if why.is_empty() { "PASS" } else { "FAIL" },
                    phase,
                    name,
                    score,
                    if wrote_devanagari { " (model wrote Devanagari)" } else { "" },
                    ms as f64 / 1000.0,
                    why.first().map(|w| format!(" | {w}")).unwrap_or_default()
                );
                rows.push(json!({
                    "case": name,
                    "phase": phase,
                    "pass": why.is_empty(),
                    "reasons": why,
                    "overlap": (score * 100.0).round() / 100.0,
                    "model_wrote_devanagari": wrote_devanagari,
                    "ms": ms,
                    "prompt_tokens": st.prompt_tokens,
                    "text": c.text,
                    "reference": c.reference,
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
            let mut ms: Vec<u64> = mine.iter().filter_map(|r| r["ms"].as_u64()).collect();
            ms.sort_unstable();
            json!({
                "phase": p,
                "cases": mine.len(),
                "passed": mine.iter().filter(|r| r["pass"] == true).count(),
                "mean_overlap": (mine.iter().filter_map(|r| r["overlap"].as_f64()).sum::<f64>() / n * 100.0).round() / 100.0,
                "model_wrote_devanagari": mine.iter().filter(|r| r["model_wrote_devanagari"] == true).count(),
                "median_ms": ms.get(ms.len() / 2).copied().unwrap_or(0),
            })
        })
        .collect();
    json!({ "phases": per_phase, "rows": rows })
}

/// hinglish.md: a table per model and phase, then each case's result.
pub fn markdown(results: &[(String, Value)]) -> String {
    let mut out = String::from(
        "# Hinglish\n\nRules: Yap's own spelling-out alone (cleanup off). Devanagari: cleanup given Whisper's Devanagari. Latin: cleanup given the rules' Latin letters (what a dictation does). Overlap: the share of the reference's words in the result.\n\n| Model | Phase | Passed | Mean overlap | Model wrote Devanagari | Median ms |\n|---|---|---|---|---|---|\n",
    );
    for (label, r) in results {
        for p in r["phases"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "| {label} | {} | {}/{} | {} | {} | {} |\n",
                p["phase"].as_str().unwrap_or(""),
                p["passed"],
                p["cases"],
                p["mean_overlap"],
                p["model_wrote_devanagari"],
                p["median_ms"],
            ));
        }
    }
    for (label, r) in results {
        out.push_str(&format!("\n## {label}\n"));
        for row in r["rows"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "\n### {} ({}) — {}, overlap {}\n",
                row["case"].as_str().unwrap_or(""),
                row["phase"].as_str().unwrap_or(""),
                if row["pass"] == true { "PASS" } else { "FAIL" },
                row["overlap"],
            ));
            for why in row["reasons"].as_array().into_iter().flatten() {
                out.push_str(&format!("- {}\n", why.as_str().unwrap_or("")));
            }
            out.push_str(&format!(
                "\nWhisper:\n\n```\n{}\n```\n\nReference:\n\n```\n{}\n```\n\nResult:\n\n```\n{}\n```\n",
                row["text"].as_str().unwrap_or(""),
                row["reference"].as_str().unwrap_or(""),
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
    fn the_cases_read_and_the_rules_alone_pass_most_of_them() {
        let cases = load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/hinglish/cases.json"), &[]).unwrap();
        assert!(cases.len() >= 8);
        let mut passed = 0;
        for c in &cases {
            assert!(hinglish::has_devanagari(&c.text), "{}", c.name);
            assert!(!hinglish::has_devanagari(&c.reference), "{}", c.name);
            assert!(!c.hindi.is_empty(), "{}", c.name);
            assert!(c.app.is_empty() || Category::from_id(&c.app).is_some(), "{}", c.name);
            let out = hinglish::to_latin(&c.text);
            if judge(c, &out).is_empty() {
                passed += 1;
            }
            assert!(overlap(&c.reference, &out) >= 0.6, "{}: {out}", c.name);
        }
        assert!(passed * 4 >= cases.len() * 3, "{passed} of {}", cases.len());
    }

    #[test]
    fn overlap_counts_the_reference_words_found() {
        assert_eq!(overlap("kal ki meeting", "Kal ki meeting."), 1.0);
        assert_eq!(overlap("hai hai", "hai"), 0.5);
        assert_eq!(overlap("", "x"), 1.0);
    }
}

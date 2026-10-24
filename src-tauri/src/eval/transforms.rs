//! `--transforms`: **saved transforms** (`transforms.rs`) on each
//! `--model`, through the app's own call — `llm::transform` (Yap's
//! guardrails, the built-in's instruction as the app sends it) and
//! `transforms::clean_output` — over `evals/transforms/cases.json`: each
//! case a transform and a text, as a person would select it, and what the
//! result must be:
//!
//! - `keeps`: words it must have exactly as written (names, numbers, dates,
//!   links: Translate must keep them);
//! - `not_contains`: words that mean the model did the wrong thing (an
//!   answer to a question "Make it a prompt" was to rewrite, a reply to a
//!   message, a preamble), case-insensitive;
//! - `contains_any`: at least one of these (the target language's common
//!   words; a prompt's "write" / "I need"…);
//! - `max_ratio` / `min_ratio`: its words against the text's (Shorter at
//!   most ~0.75; Polish and Fix grammar neither gutted nor padded);
//! - `same`: it must come back unchanged (Fix grammar on correct text).
//!
//! Every result also must not be empty, still hold `<text>` tags, or open
//! with a preamble the app would have to strip (counted as `stripped`:
//! the app takes it off, but a model that does it is noted).
//!
//! `transforms.md` in the run folder; the numbers in `summary.json`
//! (`models[].transforms`).

use std::path::Path;
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};

use super::record;
use crate::transforms::{self, Transform};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    /// A built-in's kind ("polish", "translate"…), or "custom" with
    /// `instruction`.
    pub transform: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub instruction: String,
    pub text: String,
    #[serde(default)]
    pub expect: Expect,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    #[serde(default)]
    pub keeps: Vec<String>,
    /// Each group: one of these must be in it as written (an amount in the
    /// language's own format: "€2,750" or "2 750 €").
    #[serde(default)]
    pub keeps_one_of: Vec<Vec<String>>,
    #[serde(default)]
    pub not_contains: Vec<String>,
    #[serde(default)]
    pub contains_any: Vec<String>,
    #[serde(default)]
    pub max_ratio: Option<f64>,
    #[serde(default)]
    pub min_ratio: Option<f64>,
    #[serde(default)]
    pub same: bool,
}

pub fn load(path: &Path, only: &[String]) -> Result<Vec<Case>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    let mut cases: Vec<Case> = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let only: Vec<String> = only.iter().map(|s| s.to_lowercase()).collect();
    cases.retain(|c| only.is_empty() || only.iter().any(|o| c.name.to_lowercase().contains(o) || c.transform == *o));
    if cases.is_empty() {
        return Err(format!("no transform cases in {}", path.display()));
    }
    Ok(cases)
}

pub struct Endpoint<'a> {
    pub base_url: &'a str,
    pub key: &'a str,
    pub model: &'a str,
    pub provider: &'a str,
}

fn transform_of(c: &Case) -> Transform {
    if c.transform == "custom" {
        Transform { id: "t-eval".into(), name: c.name.clone(), instruction: c.instruction.clone(), ..Default::default() }
    } else {
        Transform { id: c.transform.clone(), builtin: c.transform.clone(), language: c.language.clone(), ..Default::default() }
    }
}

fn words(s: &str) -> usize {
    s.split_whitespace().count()
}

/// Why `out` (as the app would paste it; `raw` as the model wrote it)
/// fails `c`, if it does.
pub fn judge(c: &Case, raw: &str, out: &str) -> Vec<String> {
    let mut why = Vec::new();
    let e = &c.expect;
    if out.trim().is_empty() {
        why.push("empty".into());
        return why;
    }
    if out.contains("<text>") || out.contains("</text>") {
        why.push("kept the <text> tags".into());
    }
    let lower = out.to_lowercase();
    for k in &e.keeps {
        if !out.contains(k.as_str()) {
            why.push(format!("lost {k:?}"));
        }
    }
    for group in &e.keeps_one_of {
        // A no-break space is a space here (French writes "2 750 €" with one).
        let spaced = out.replace(['\u{a0}', '\u{202f}'], " ");
        if !group.iter().any(|k| spaced.contains(k.as_str())) {
            why.push(format!("lost all of {group:?}"));
        }
    }
    for n in &e.not_contains {
        if found(&lower, &n.to_lowercase()) {
            why.push(format!("says {n:?}"));
        }
    }
    if !e.contains_any.is_empty() && !e.contains_any.iter().any(|w| found(&lower, &w.to_lowercase())) {
        why.push(format!("none of {:?}", e.contains_any));
    }
    let ratio = words(out) as f64 / words(&c.text).max(1) as f64;
    if let Some(max) = e.max_ratio {
        if ratio > max {
            why.push(format!("{ratio:.2}× the words (at most {max})"));
        }
    }
    if let Some(min) = e.min_ratio {
        if ratio < min {
            why.push(format!("{ratio:.2}× the words (at least {min})"));
        }
    }
    if e.same && !crate::as_said::same_text(out.trim(), c.text.trim()) {
        why.push("changed text that had no mistakes".into());
    }
    let _ = raw;
    why
}

/// `needle` in `text`, case already folded: a word or phrase of plain
/// letters as a whole word ("la" isn't in "hablar"), anything else (CJK,
/// punctuation, "import ") as it is.
fn found(text: &str, needle: &str) -> bool {
    let plain = needle.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '\'' || c == '-');
    if plain && !needle.trim().is_empty() && needle.trim() == needle {
        has_word(text, needle)
    } else {
        text.contains(needle)
    }
}

/// `word` in `text` as a whole word (or a phrase), case already folded.
fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// Every case `repeat` times on one model; its results for summary.json and
/// transforms.md.
pub async fn run(cases: &[Case], ep: &Endpoint<'_>, repeat: u32) -> Value {
    let mut rows = Vec::new();
    let (mut passed, mut stripped) = (0usize, 0usize);
    let mut ms_all: Vec<u64> = Vec::new();
    let mut by_kind: std::collections::BTreeMap<String, (usize, usize)> = std::collections::BTreeMap::new();
    let mut tokens = 0u64;
    for c in cases {
        let t = transform_of(c);
        for rep in 0..repeat.max(1) {
            let name = if repeat > 1 { format!("{}#{}", c.name, rep + 1) } else { c.name.clone() };
            record::set_context(json!({ "model": ep.model, "scenario": name }));
            record::begin_turn();
            let t0 = Instant::now();
            let asked = crate::llm::transform(&t.instruction(), &c.text, ep.base_url, ep.key, ep.model, ep.provider, false).await;
            let ms = t0.elapsed().as_millis() as u64;
            let stats = record::end_turn();
            tokens += stats.total_tokens;
            let (raw, error) = match asked {
                Ok(r) => (r, None),
                Err(e) => (String::new(), Some(record::redact(&e))),
            };
            let out = transforms::clean_output(&raw);
            let mut why = judge(c, &raw, &out);
            if let Some(e) = &error {
                why.insert(0, format!("the call failed: {e}"));
            }
            let was_stripped = !raw.trim().is_empty() && out != raw.trim();
            stripped += usize::from(was_stripped);
            let pass = why.is_empty();
            passed += usize::from(pass);
            let k = by_kind.entry(c.transform.clone()).or_default();
            k.1 += 1;
            k.0 += usize::from(pass);
            if error.is_none() {
                ms_all.push(ms);
            }
            println!(
                "  {:<4} {:<36} {:.1} s{}{}",
                if pass { "PASS" } else { "FAIL" },
                name,
                ms as f64 / 1000.0,
                if was_stripped { " (preamble stripped)" } else { "" },
                why.first().map(|w| format!(" | {w}")).unwrap_or_default()
            );
            rows.push(json!({
                "case": name,
                "transform": t.name(),
                "status": if pass { "pass" } else { "fail" },
                "reasons": why,
                "ms": ms,
                "prompt_tokens": stats.prompt_tokens,
                "completion_tokens": stats.completion_tokens,
                "stripped": was_stripped,
                "text": c.text,
                "output": record::redact(&out),
                "raw": if was_stripped { json!(record::redact(&raw)) } else { Value::Null },
            }));
        }
    }
    ms_all.sort_unstable();
    let median = ms_all.get(ms_all.len() / 2).copied().unwrap_or(0);
    json!({
        "cases": rows.len(),
        "passed": passed,
        "stripped": stripped,
        "tokens": tokens,
        "median_ms": median,
        "max_ms": ms_all.last().copied().unwrap_or(0),
        "by_transform": by_kind.iter().map(|(k, (p, n))| (k.clone(), json!(format!("{p}/{n}")))).collect::<serde_json::Map<_, _>>(),
        "rows": rows,
    })
}

/// transforms.md: a table per model, then each case's text and result.
pub fn markdown(results: &[(String, Value)]) -> String {
    let mut out = String::from("# Saved transforms\n\n| Model | Passed | Per transform | Preamble stripped | Median / slowest | Tokens |\n|---|---|---|---|---|---|\n");
    for (label, r) in results {
        let per: Vec<String> = r["by_transform"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| format!("{k} {}", v.as_str().unwrap_or("")))
            .collect();
        out.push_str(&format!(
            "| {label} | {}/{} | {} | {} | {:.1} / {:.1} s | {} |\n",
            r["passed"],
            r["cases"],
            per.join(", "),
            r["stripped"],
            r["median_ms"].as_f64().unwrap_or(0.0) / 1000.0,
            r["max_ms"].as_f64().unwrap_or(0.0) / 1000.0,
            r["tokens"],
        ));
    }
    for (label, r) in results {
        out.push_str(&format!("\n## {label}\n"));
        for row in r["rows"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "\n### {} ({}) — {}\n",
                row["case"].as_str().unwrap_or(""),
                row["transform"].as_str().unwrap_or(""),
                row["status"].as_str().unwrap_or("").to_uppercase()
            ));
            for why in row["reasons"].as_array().into_iter().flatten() {
                out.push_str(&format!("- {}\n", why.as_str().unwrap_or("")));
            }
            out.push_str(&format!(
                "\nText:\n\n```\n{}\n```\n\nResult:\n\n```\n{}\n```\n",
                row["text"].as_str().unwrap_or(""),
                row["output"].as_str().unwrap_or("")
            ));
            if let Some(raw) = row["raw"].as_str() {
                out.push_str(&format!("\nThe model wrote (before Yap stripped it):\n\n```\n{raw}\n```\n"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(transform: &str, text: &str, expect: Expect) -> Case {
        Case { name: "t".into(), transform: transform.into(), language: String::new(), instruction: String::new(), text: text.into(), expect }
    }

    #[test]
    fn the_cases_file_reads_and_every_transform_is_one_yap_has() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/transforms/cases.json");
        let cases = load(&path, &[]).unwrap();
        assert!(cases.len() >= 15, "{}", cases.len());
        for c in &cases {
            let t = transform_of(c);
            assert!(t.usable(), "{}: {}", c.name, c.transform);
        }
        for kind in ["polish", "shorter", "grammar", "translate", "prompt"] {
            assert!(cases.iter().filter(|c| c.transform == kind).count() >= 3, "{kind}");
        }
    }

    #[test]
    fn a_result_is_judged_by_what_it_keeps_says_and_how_long_it_is() {
        let e = Expect {
            keeps: vec!["Priya".into(), "£1,200".into()],
            not_contains: vec!["Here is".into()],
            contains_any: vec!["el".into(), "la".into()],
            max_ratio: Some(2.0),
            ..Default::default()
        };
        let c = case("translate", "Tell Priya the quote is £1,200.", e);
        assert!(judge(&c, "", "Dile a Priya que el presupuesto es £1,200.").is_empty());
        let why = judge(&c, "", "Here is it: Dile a Pria que presupuesto es 1200.");
        assert_eq!(why.len(), 4, "{why:?}");
        // A word, not part of one: "la" isn't found in "hablar".
        let c = case("translate", "x", Expect { contains_any: vec!["la".into()], ..Default::default() });
        assert!(!judge(&c, "", "hablar").is_empty());
        // An amount in the language's own format, a no-break space in it.
        let amount = case("translate", "x", Expect { keeps_one_of: vec![vec!["€2,750".into(), "2 750 €".into()]], ..Default::default() });
        assert!(judge(&amount, "", "pour 2\u{202f}750 € le 14 mars").is_empty());
        assert!(judge(&amount, "", "for €2,750").is_empty());
        assert!(!judge(&amount, "", "pour 2 570 €").is_empty());
        let same = case("grammar", "It's done.", Expect { same: true, ..Default::default() });
        assert!(judge(&same, "", "It's done.").is_empty());
        assert!(!judge(&same, "", "It is done.").is_empty());
        assert_eq!(judge(&same, "", "  "), ["empty"]);
    }
}

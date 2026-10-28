//! `--vision`: **screenshot questions** (Snip to Chat) on each `--model`,
//! through Chat's own path — the screenshot stored as a snip is
//! (`snip::images`), then the turn `chat_send` runs (`turn::chat_turn`
//! inside `snip::turn::with`) — over `evals/vision/cases.json`: made-up
//! screenshots (a till receipt, an error dialog, a bar chart with no
//! numbers on it, a page of small print, a settings page whose switches
//! say what's on, an editor's failing test; `evals/vision/src/*.html`,
//! drawn by Edge) and a question each, two ways:
//!
//! - **picture**: the screenshot as an `image_url` part, fitted for the
//!   provider (`snip::turn::prepare`: a model on this PC gets a 1,024 px
//!   long edge). A local model's server gets its projector (`--mmproj`,
//!   `local::projector`).
//! - **text**: "Send its text instead": Windows' OCR on this PC
//!   (`snip::ocr::question_with_text`, the real engine) after the question.
//!
//! An answer passes when each `expect.all` group has one of its words
//! (case folded; a plain word whole) and none of `expect.not`.
//! `vision.md` in the run folder; the numbers in `summary.json`
//! (`models[].vision`).

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};

use super::record;

/// The two ways a screenshot goes.
pub const PATHS: [&str; 2] = ["picture", "text"];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    /// A PNG beside the cases file.
    pub image: String,
    pub question: String,
    #[serde(default)]
    pub expect: Expect,
    /// What it tests ("text", "layout", "visual"): the table's columns.
    #[serde(default)]
    pub kind: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    /// Each group: one of these must be in the answer.
    #[serde(default)]
    pub all: Vec<Vec<String>>,
    /// None of these may be.
    #[serde(default)]
    pub not: Vec<String>,
}

pub struct Cases {
    pub dir: PathBuf,
    pub cases: Vec<Case>,
}

pub fn load(path: &Path, only: &[String]) -> Result<Cases, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    let mut cases: Vec<Case> = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let only: Vec<String> = only.iter().map(|s| s.to_lowercase()).collect();
    cases.retain(|c| only.is_empty() || only.iter().any(|o| c.name.to_lowercase().contains(o) || c.kind == *o));
    if cases.is_empty() {
        return Err(format!("no screenshot cases in {}", path.display()));
    }
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    for c in &cases {
        if !dir.join(&c.image).is_file() {
            return Err(format!("{}: no {}", c.name, dir.join(&c.image).display()));
        }
    }
    Ok(Cases { dir, cases })
}

/// Why `answer` fails `c`, if it does. Pure.
pub fn judge(c: &Case, answer: &str) -> Vec<String> {
    let lower = answer.to_lowercase();
    let mut why = Vec::new();
    if lower.trim().is_empty() {
        return vec!["empty".into()];
    }
    for group in &c.expect.all {
        if !group.iter().any(|w| found(&lower, &w.to_lowercase())) {
            why.push(format!("none of {group:?}"));
        }
    }
    for n in &c.expect.not {
        if found(&lower, &n.to_lowercase()) {
            why.push(format!("says {n:?}"));
        }
    }
    why
}

/// `needle` in `text`, case already folded: plain letters and digits as a
/// whole word ("off" isn't in "office"), anything else as it is.
fn found(text: &str, needle: &str) -> bool {
    let plain = needle.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ');
    if plain && !needle.trim().is_empty() {
        text.match_indices(needle).any(|(i, _)| {
            let before = text[..i].chars().next_back();
            let after = text[i + needle.len()..].chars().next();
            !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
        })
    } else {
        text.contains(needle)
    }
}

/// The settings a question runs on: the Chat scope on the model.
pub fn settings(provider: &str, base_url: &str, key: &str, model: &str) -> crate::config::YapConfig {
    let mut cfg = crate::config::YapConfig {
        pp_provider: provider.to_string(),
        pp_base_url: base_url.to_string(),
        pp_api_key: key.to_string(),
        pp_model: model.to_string(),
        ..Default::default()
    };
    cfg.llm_scopes.insert(
        "chat".to_string(),
        crate::config::LlmScope {
            enabled: true,
            provider: provider.to_string(),
            base_url: base_url.to_string(),
            model: model.to_string(),
            api_key: key.to_string(),
            prompt: super::runner::DEFAULT_PERSONA.to_string(),
            ..Default::default()
        },
    );
    cfg
}

pub struct Endpoint<'a> {
    pub base_url: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
}

/// One question one way: the reply (or the error), its time and tokens.
async fn ask(c: &Case, png: &[u8], path: &str, ep: &Endpoint<'_>) -> (Result<String, String>, u64, record::TurnStats) {
    crate::chat::chats::eval_reset();
    let t0 = Instant::now();
    let asked = async {
        let (w, h) = crate::snip::images::png_size(png).ok_or("not a PNG")?;
        let saved = crate::snip::images::save(png, w, h)?;
        let ids = vec![saved.id];
        let (question, turn_images) = if path == "text" {
            let (q, images) = crate::snip::ocr::question_with_text(&ids, &c.question).await?;
            (q, crate::snip::turn::TurnImages { images, ..Default::default() })
        } else {
            (c.question.clone(), crate::snip::turn::prepare(&ids, ep.provider, ep.base_url).await?)
        };
        let turn = crate::snip::turn::with(
            turn_images,
            Box::pin(crate::chat::turn::chat_turn(None, None, &question, None, false, |_, _| {}, None, None)),
        )
        .await?;
        Ok::<String, String>(turn.reply)
    };
    record::begin_turn();
    let out = asked.await;
    let stats = record::end_turn();
    (out, t0.elapsed().as_millis() as u64, stats)
}

/// Every case each way in `paths`, `repeat` times, on one model (its
/// settings already in place).
pub async fn run(cases: &Cases, ep: &Endpoint<'_>, paths: &[&str], repeat: u32) -> Value {
    let mut rows = Vec::new();
    let mut per: std::collections::BTreeMap<String, (usize, usize, Vec<u64>, u64)> = Default::default();
    for c in &cases.cases {
        let png = match std::fs::read(cases.dir.join(&c.image)) {
            Ok(p) => p,
            Err(e) => {
                rows.push(json!({ "case": c.name, "error": e.to_string() }));
                continue;
            }
        };
        for path in paths {
            for rep in 0..repeat.max(1) {
                let name = if repeat > 1 { format!("{}#{}", c.name, rep + 1) } else { c.name.clone() };
                record::set_context(json!({ "model": ep.model, "scenario": format!("vision {path}: {name}") }));
                let (out, ms, stats) = ask(c, &png, path, ep).await;
                let (answer, error) = match out {
                    Ok(a) => (a, None),
                    Err(e) => (String::new(), Some(record::redact(&e))),
                };
                let mut why = judge(c, &answer);
                if let Some(e) = &error {
                    why.insert(0, format!("it failed: {e}"));
                }
                let pass = why.is_empty();
                let p = per.entry(path.to_string()).or_default();
                p.1 += 1;
                p.0 += usize::from(pass);
                if error.is_none() {
                    p.2.push(ms);
                }
                p.3 += stats.prompt_tokens;
                println!(
                    "  {:<4} {:<8} {:<22} {:.1} s, {} prompt tokens{}",
                    if pass { "PASS" } else { "FAIL" },
                    path,
                    name,
                    ms as f64 / 1000.0,
                    stats.prompt_tokens,
                    why.first().map(|w| format!(" | {}", w.chars().take(120).collect::<String>())).unwrap_or_default()
                );
                rows.push(json!({
                    "case": name,
                    "kind": c.kind,
                    "path": path,
                    "question": c.question,
                    "status": if pass { "pass" } else { "fail" },
                    "reasons": why,
                    "ms": ms,
                    "requests": stats.requests,
                    "prompt_tokens": stats.prompt_tokens,
                    "completion_tokens": stats.completion_tokens,
                    "answer": record::redact(&answer),
                }));
            }
        }
    }
    let by_path: serde_json::Map<String, Value> = per
        .into_iter()
        .map(|(k, (passed, n, mut ms, prompt))| {
            ms.sort_unstable();
            let median = ms.get(ms.len() / 2).copied().unwrap_or(0);
            (
                k,
                json!({
                    "passed": passed,
                    "cases": n,
                    "median_ms": median,
                    "max_ms": ms.last().copied().unwrap_or(0),
                    "mean_prompt_tokens": prompt / n.max(1) as u64,
                }),
            )
        })
        .collect();
    json!({ "by_path": by_path, "rows": rows })
}

/// vision.md: a table per model and way, then each answer.
pub fn markdown(results: &[(String, Value)]) -> String {
    let mut out = String::from(
        "# Screenshot questions\n\n| Model | Way | Passed | Median / slowest | Prompt tokens (mean) |\n|---|---|---|---|---|\n",
    );
    for (label, r) in results {
        for (path, p) in r["by_path"].as_object().into_iter().flatten() {
            out.push_str(&format!(
                "| {label} | {path} | {}/{} | {:.1} / {:.1} s | {} |\n",
                p["passed"],
                p["cases"],
                p["median_ms"].as_f64().unwrap_or(0.0) / 1000.0,
                p["max_ms"].as_f64().unwrap_or(0.0) / 1000.0,
                p["mean_prompt_tokens"],
            ));
        }
    }
    for (label, r) in results {
        out.push_str(&format!("\n## {label}\n"));
        for row in r["rows"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "\n### {} ({}) — {}, {:.1} s\n\n> {}\n",
                row["case"].as_str().unwrap_or(""),
                row["path"].as_str().unwrap_or(""),
                row["status"].as_str().unwrap_or("").to_uppercase(),
                row["ms"].as_f64().unwrap_or(0.0) / 1000.0,
                row["question"].as_str().unwrap_or(""),
            ));
            for why in row["reasons"].as_array().into_iter().flatten() {
                out.push_str(&format!("- {}\n", why.as_str().unwrap_or("")));
            }
            out.push_str(&format!("\n```\n{}\n```\n", row["answer"].as_str().unwrap_or("")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cases_file_reads_and_every_screenshot_is_there() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/vision/cases.json");
        let cases = load(&path, &[]).unwrap();
        assert!(cases.cases.len() >= 8, "{}", cases.cases.len());
        for c in &cases.cases {
            assert!(!c.expect.all.is_empty(), "{}: nothing to check", c.name);
            let png = std::fs::read(cases.dir.join(&c.image)).unwrap();
            assert!(crate::snip::images::png_size(&png).is_some(), "{}", c.image);
        }
        for kind in ["text", "visual"] {
            assert!(cases.cases.iter().any(|c| c.kind == kind), "{kind}");
        }
    }

    #[test]
    fn an_answer_is_judged_by_its_words() {
        let c = Case {
            name: "t".into(),
            image: String::new(),
            question: String::new(),
            kind: String::new(),
            expect: Expect { all: vec![vec!["september".into()], vec!["£5".into(), "5 pounds".into()]], not: vec!["is on".into()] },
        };
        assert!(judge(&c, "September, and a card is £5.").is_empty());
        assert_eq!(judge(&c, "September; 5 pounds; Bluetooth is on").len(), 1);
        assert_eq!(judge(&c, "  "), ["empty"]);
        // A word whole: "off" isn't in "office".
        let off = Case { expect: Expect { all: vec![vec!["off".into()]], not: vec![] }, ..c };
        assert!(!judge(&off, "The office is open").is_empty());
        assert!(judge(&off, "Bluetooth is off.").is_empty());
    }
}
